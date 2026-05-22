use actix_web::{web, HttpResponse};

use super::browser_lifecycle::{force_close_browser, force_close_browser_session};
use super::image_output::process_poll_images;
use super::image_refs::fetch_ref_images_as_data_urls;
use super::{error_response, ImageGenContext, OpenAiImageRequest};
use crate::account_manager::handle_task_failure;
use crate::orchestrator::Orchestrator;
use crate::sync_ext::MutexExt;
use crate::types::{AllocationResult, TaskKind, TaskRecord, TaskStatus};
use crate::AppState;

/// 准备阶段的错误：HTTP 状态码、错误 code、错误详情。
pub(crate) type PrepareError = (u16, &'static str, String);

/// 把 OpenAI 风格请求转换为插件 flow 需要的运行上下文。
pub(crate) async fn prepare_image_context(
    state: &web::Data<AppState>,
    body: &OpenAiImageRequest,
    request_id: String,
) -> Result<ImageGenContext, PrepareError> {
    let (manifest, model_info) =
        state
            .plugin_loader
            .resolve_model(&body.model)
            .ok_or_else(|| {
                (
                    404u16,
                    "model_not_found",
                    format!("模型 {} 不存在", body.model),
                )
            })?;
    let platform = manifest.platform_id.clone();

    let flow = manifest
        .flows
        .get("generate_image")
        .cloned()
        .ok_or_else(|| {
            (
                500u16,
                "flow_not_found",
                format!("{} 未配置 generate_image flow", platform),
            )
        })?;

    let (account_id, profile_dir, account_cookies) = allocate_account(state, &platform)?;
    let chrome_path = {
        let storage = state.storage.lock_recover();
        crate::resolve_chrome_path(&storage)
    };
    let scripts_dir = state
        .plugin_loader
        .get_plugin_dir(&platform)
        .map(|d| d.join("scripts").to_string_lossy().to_string())
        .unwrap_or_default();

    // 编辑接口的输入图既给浏览器文件选择器使用，也转成 data URL 供脚本绕开 CORS。
    let ref_image_urls = body.reference_urls();
    let ref_image_locals = super::image_output::download_ref_images(&ref_image_urls).await;
    let ref_image_local = ref_image_locals.first().cloned().unwrap_or_default();
    let ref_image_dataurls = fetch_ref_images_as_data_urls(&ref_image_urls).await;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let workspace_dir = ensure_workspace_dir(&state.app_data_dir, &today, &platform);
    let browser_session_id = format!("img-{}", request_id);

    let prompt = body.prompt_text();
    let image_aspect_ratio = body.image_aspect_ratio();
    let max_images = body.max_images();
    let output_format = body.output_format();
    let quality = body.quality();
    let size = body.requested_size();

    let initial_context = serde_json::json!({
        "account_id": account_id,
        "request_id": request_id,
        "profile_dir": profile_dir,
        "browser_session_id": browser_session_id,
        "chrome_path": chrome_path.unwrap_or_default(),
        "prompt": prompt,
        "image_aspect_ratio": image_aspect_ratio,
        "max_images": max_images,
        "model_id": body.model,
        "model_display_name": model_info.display_name,
        "scripts_dir": scripts_dir,
        "account_cookies": account_cookies,
        "ref_image_local": ref_image_local,
        "ref_image_locals": ref_image_locals,
        "ref_image_urls": ref_image_urls,
        "ref_image_dataurls": ref_image_dataurls,
        "workspace_dir": workspace_dir.to_string_lossy(),
        "output_format": output_format,
        "quality": quality,
        "size": size,
    });

    Ok(ImageGenContext {
        platform,
        model_display_name: model_info.display_name.clone(),
        flow,
        account_id: account_id.clone(),
        profile_dir: profile_dir.clone(),
        account_cookies: account_cookies.clone(),
        request_id,
        browser_session_id,
        model_id: body.model.clone(),
        prompt,
        image_aspect_ratio,
        ref_image_local,
        ref_image_locals,
        workspace_dir,
        initial_context,
        output_format,
        quality,
        size,
    })
}

pub(crate) async fn generate_image_json(
    state: web::Data<AppState>,
    ctx: ImageGenContext,
) -> HttpResponse {
    insert_processing_task(&state, &ctx);

    let result = Orchestrator::execute_flow(
        &ctx.flow,
        &state.engine_registry,
        ctx.initial_context.clone(),
    )
    .await;

    cleanup_reference_files(&ctx);
    release_account_and_update_usage(&state, &ctx, &result);

    force_close_browser(
        &state.engine_registry,
        &state.plugin_loader,
        &ctx.platform,
        &ctx.profile_dir,
    )
    .await;

    match result {
        Ok(results) => {
            let poll_data = find_poll_data(&results);
            let image_results = process_poll_images(poll_data, &ctx, &results).await;
            if image_results.is_empty() {
                mark_task_failed(&state, &ctx, "生成完成但没有拿到图片结果");
                return error_response(500, "generation_failed", "生成完成但没有拿到图片结果");
            }

            let first = image_results.first();
            let task_json = serde_json::json!({
                "width": first.map(|r| r.width).unwrap_or(0),
                "height": first.map(|r| r.height).unwrap_or(0),
                "imageCount": image_results.len(),
                "source": poll_data.and_then(|d| d["source"].as_str()).unwrap_or("unknown"),
            });
            mark_task_completed(&state, &ctx, &task_json);

            let data: Vec<serde_json::Value> = image_results
                .into_iter()
                .map(|item| serde_json::json!({ "b64_json": item.b64_json }))
                .collect();

            HttpResponse::Ok().json(serde_json::json!({
                "created": chrono::Utc::now().timestamp(),
                "data": data,
                "output_format": ctx.output_format,
                "quality": ctx.quality,
                "size": ctx.size,
            }))
        }
        Err(e) => {
            tracing::error!(error = %e, platform = %ctx.platform, "图片生成失败");
            force_close_browser_session(&state.engine_registry, &ctx.browser_session_id).await;
            mark_task_failed(&state, &ctx, &e.to_string());
            {
                let storage = state.storage.lock_recover();
                handle_task_failure(&storage, &state.plugin_loader, &ctx.account_id, &e, &[]);
            }
            if crate::account_verification::error_suggests_verification(&e.to_string())
                && !crate::account_verification::is_verification_task(&ctx)
            {
                crate::account_verification::spawn(state.clone().into_inner(), ctx.clone());
            }
            error_response(500, "generation_failed", &e.to_string())
        }
    }
}

fn allocate_account(
    state: &web::Data<AppState>,
    platform: &str,
) -> Result<(String, String, String), PrepareError> {
    let storage = state.storage.lock_recover();
    if let Err(e) = storage.recover_expired_cooldowns() {
        tracing::warn!(error = %e, "恢复冷却账号失败");
    }
    let accounts = storage
        .list_accounts_by_platform(platform)
        .map_err(|e| (500u16, "storage_error", e.to_string()))?;
    let mut scheduler = state.scheduler.lock_recover();
    match scheduler.allocate_with_reason(platform, &accounts, 86400) {
        AllocationResult::Allocated(id) => {
            let acc = accounts.iter().find(|a| a.id == id).unwrap();
            scheduler.increment_active(&id);
            Ok((id, acc.profile_dir.clone(), acc.cookies.clone()))
        }
        AllocationResult::NoAccounts => Err((
            503u16,
            "no_accounts",
            format!("{} 平台没有可用账号", platform),
        )),
        AllocationResult::AllBusy => {
            Err((503u16, "all_busy", format!("{} 平台所有账号忙碌", platform)))
        }
        AllocationResult::AllQuotaExhausted => Err((
            503u16,
            "quota_exhausted",
            format!("{} 平台所有账号配额耗尽", platform),
        )),
    }
}

fn ensure_workspace_dir(
    app_data_dir: &std::path::Path,
    today: &str,
    platform: &str,
) -> std::path::PathBuf {
    let workspace_dir = app_data_dir.join("workspace").join(today).join(platform);
    if std::fs::create_dir_all(&workspace_dir).is_ok() {
        return workspace_dir;
    }
    let fallback = std::env::temp_dir()
        .join("frame64/workspace")
        .join(today)
        .join(platform);
    std::fs::create_dir_all(&fallback).ok();
    fallback
}

fn insert_processing_task(state: &web::Data<AppState>, ctx: &ImageGenContext) {
    let storage = state.storage.lock_recover();
    let task = TaskRecord {
        id: ctx.request_id.clone(),
        platform: ctx.platform.clone(),
        task_kind: TaskKind::Image,
        model_id: ctx.model_id.clone(),
        prompt: ctx.prompt.clone(),
        account_id: Some(ctx.account_id.clone()),
        status: TaskStatus::Processing,
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: chrono::Utc::now(),
        completed_at: None,
    };
    if let Err(e) = storage.insert_task(&task) {
        tracing::error!(error = %e, "保存图片任务记录失败");
    }
}

fn cleanup_reference_files(ctx: &ImageGenContext) {
    for p in std::iter::once(ctx.ref_image_local.as_str())
        .chain(ctx.ref_image_locals.iter().map(String::as_str))
    {
        if p.is_empty() {
            continue;
        }
        if let Err(e) = std::fs::remove_file(p) {
            tracing::debug!(path = %p, error = %e, "清理参考图临时文件失败");
        }
    }
}

fn release_account_and_update_usage(
    state: &web::Data<AppState>,
    ctx: &ImageGenContext,
    result: &Result<Vec<crate::types::StepResult>, crate::error::AppError>,
) {
    state
        .scheduler
        .lock_recover()
        .decrement_active(&ctx.account_id);

    let storage = state.storage.lock_recover();
    if let Ok(Some(mut acc)) = storage.get_account(&ctx.account_id) {
        acc.used_today += 1;
        acc.used_total += 1;
        let _ = storage.update_usage(&ctx.account_id, acc.used_today, acc.used_total);
        if let Ok(results) = result {
            if let Some(close_result) = results
                .iter()
                .rev()
                .find(|r| r.data.get("cookies_json").is_some())
            {
                if let Some(cj) = close_result.data["cookies_json"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                {
                    acc.cookies = cj.to_string();
                    let _ = storage.update_account(&acc);
                }
            }
        }
    }
}

fn find_poll_data(results: &[crate::types::StepResult]) -> Option<&serde_json::Value> {
    results
        .iter()
        .rev()
        .find(|r| {
            r.data.get("imageCount").is_some()
                || r.data.get("images").is_some()
                || r.data.get("download_url").is_some()
        })
        .map(|r| &r.data)
}

fn mark_task_completed(
    state: &web::Data<AppState>,
    ctx: &ImageGenContext,
    result_json: &serde_json::Value,
) {
    let storage = state.storage.lock_recover();
    let _ = storage.update_task_status(&ctx.request_id, TaskStatus::Completed, None, None);
    let _ = storage.update_task_result_data(&ctx.request_id, &result_json.to_string());
}

fn mark_task_failed(state: &web::Data<AppState>, ctx: &ImageGenContext, message: &str) {
    let storage = state.storage.lock_recover();
    let _ = storage.update_task_status(&ctx.request_id, TaskStatus::Failed, None, Some(message));
}
