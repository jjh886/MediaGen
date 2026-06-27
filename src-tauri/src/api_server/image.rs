use actix_web::{web, HttpResponse};

use super::image_account::{
    allocate_account_when_available, AccountQueueReporter, ActiveAccountLease, PrepareError,
};
use super::image_capabilities::validate_image_capabilities;
use super::image_partials::{format_partial_sse_event, PartialImageEvent};
use super::image_refs::fetch_ref_images_as_data_urls;
use super::{error_response, ImageEndpoint, ImageGenContext, OpenAiImageRequest};
use crate::sync_ext::MutexExt;
use crate::types::{StepResult, TaskKind, TaskRecord, TaskStatus};
use crate::AppState;

pub(crate) struct ImageApiResponse {
    pub created: i64,
    pub images: Vec<String>,
    pub partial_images: Vec<PartialImageEvent>,
    pub output_format: String,
    pub quality: String,
    pub size: String,
    pub background: Option<String>,
}

impl ImageApiResponse {
    pub(crate) fn to_json_value(&self) -> serde_json::Value {
        let data: Vec<serde_json::Value> = self
            .images
            .iter()
            .map(|b64_json| serde_json::json!({ "b64_json": b64_json }))
            .collect();
        let mut body = serde_json::json!({
            "created": self.created,
            "data": data,
            "output_format": self.output_format,
            "quality": self.quality,
            "size": self.size,
        });
        if let Some(background) = response_background(self.background.as_deref()) {
            body["background"] = serde_json::Value::String(background.to_string());
        }
        body
    }

    pub(crate) fn to_sse_body(&self, endpoint: ImageEndpoint) -> String {
        let event_type = endpoint.completed_event_type();
        let mut out = String::new();
        for partial in &self.partial_images {
            out.push_str(&format_partial_sse_event(endpoint, partial));
        }
        for b64_json in &self.images {
            let mut payload = serde_json::json!({
                "type": event_type,
                "b64_json": b64_json,
                "created_at": self.created,
                "size": self.size,
                "quality": self.quality,
                "output_format": self.output_format,
            });
            if let Some(background) = response_background(self.background.as_deref()) {
                payload["background"] = serde_json::Value::String(background.to_string());
            }
            out.push_str(&format_sse_event(event_type, &payload));
        }
        out
    }
}

fn response_background(value: Option<&str>) -> Option<&str> {
    value.filter(|background| matches!(*background, "transparent" | "opaque"))
}

fn format_sse_event(event: &str, payload: &serde_json::Value) -> String {
    format!("event: {event}\ndata: {payload}\n\n")
}

/// 把 OpenAI 风格请求转换为插件 flow 需要的运行上下文。
pub(crate) async fn prepare_image_context(
    state: &web::Data<AppState>,
    body: &OpenAiImageRequest,
    request_id: String,
    endpoint: ImageEndpoint,
    account_queue_reporter: Option<AccountQueueReporter>,
) -> Result<(ImageGenContext, ActiveAccountLease), PrepareError> {
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
    validate_image_capabilities(manifest, endpoint, body)?;
    let mask_image_url = body
        .mask
        .as_ref()
        .and_then(|item| item.image_url.clone())
        .filter(|url| !url.trim().is_empty());

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

    let (account_id, profile_dir, account_cookies, account_lease) =
        allocate_account_when_available(state, &platform, &request_id, account_queue_reporter)
            .await?;
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
    let mask_image_dataurl = fetch_mask_image_data_url(mask_image_url.as_deref()).await;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let workspace_dir = ensure_workspace_dir(&state.app_data_dir, &today, &platform);
    let browser_session_id = format!("img-{}", request_id);

    let prompt = body.prompt_text();
    let image_aspect_ratio = body.image_aspect_ratio();
    let max_images = body.max_images();
    let output_format = body.output_format();
    let quality = body.quality();
    let size = body.requested_size();
    let output_compression = body.output_compression;
    let background = body.background.clone();
    let image_endpoint = match endpoint {
        ImageEndpoint::Generation => "generation",
        ImageEndpoint::Edit => "edit",
    };
    let image_action = match endpoint {
        ImageEndpoint::Generation => "generate",
        ImageEndpoint::Edit => "edit",
    };

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
        "mask_image_url": mask_image_url,
        "mask_image_dataurl": mask_image_dataurl,
        "workspace_dir": workspace_dir.to_string_lossy(),
        "output_format": output_format,
        "quality": quality,
        "size": size,
        // 下面这些字段是 API 参数的插件桥接层；插件按需转发给真实平台。
        "image_endpoint": image_endpoint,
        "image_action": image_action,
        "partial_images": body.partial_images,
        "output_compression": output_compression,
        "background": background,
        "moderation": body.moderation,
        "input_fidelity": body.input_fidelity,
        "user": body.user,
    });

    Ok((
        ImageGenContext {
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
            output_compression,
            background,
        },
        account_lease,
    ))
}

pub(crate) async fn generate_image_json(
    state: web::Data<AppState>,
    ctx: ImageGenContext,
    account_lease: ActiveAccountLease,
) -> HttpResponse {
    match generate_image_response(state, ctx, account_lease).await {
        Ok(response) => HttpResponse::Ok().json(response.to_json_value()),
        Err((status, code, detail)) => error_response(status, code, &detail),
    }
}

pub(crate) async fn generate_image_response(
    state: web::Data<AppState>,
    ctx: ImageGenContext,
    account_lease: ActiveAccountLease,
) -> Result<ImageApiResponse, PrepareError> {
    super::image_attempt::generate_image_response_with_attempts(state, ctx, account_lease).await
}

async fn fetch_mask_image_data_url(mask_image_url: Option<&str>) -> String {
    let Some(url) = mask_image_url else {
        return String::new();
    };
    fetch_ref_images_as_data_urls(&[url.to_string()])
        .await
        .into_iter()
        .next()
        .unwrap_or_else(|| url.to_string())
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
        .join("mediagen/workspace")
        .join(today)
        .join(platform);
    std::fs::create_dir_all(&fallback).ok();
    fallback
}

pub(crate) fn insert_processing_task(state: &web::Data<AppState>, ctx: &ImageGenContext) {
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
        attempts: Vec::new(),
        created_at: chrono::Utc::now(),
        completed_at: None,
    };
    if let Err(e) = storage.insert_task(&task) {
        tracing::error!(error = %e, "保存图片任务记录失败");
    }
}

pub(crate) fn cleanup_reference_files(ctx: &ImageGenContext) {
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

pub(crate) fn release_account_and_update_usage(
    state: &web::Data<AppState>,
    ctx: &ImageGenContext,
    mut account_lease: ActiveAccountLease,
    results: Option<&[StepResult]>,
    usage_consumed: bool,
) {
    {
        let storage = state.storage.lock_recover();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        if let Err(e) = storage.refresh_stale_daily_usage(&today) {
            tracing::warn!(error = %e, "刷新账号今日用量失败");
        }
        if let Ok(Some(mut acc)) = storage.get_account(&ctx.account_id) {
            if usage_consumed {
                acc.used_today += 1;
                acc.used_total += 1;
            }
            acc.last_used_at = Some(chrono::Utc::now());
            if let Some(results) = results {
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
                    }
                }
            }
            let _ = storage.update_account(&acc);
        }
    }
    // 时间戳先落库，再释放账号并发，避免下一单绕过刚写入的任务间隔。
    account_lease.release();
}

pub(crate) fn find_poll_data(results: &[StepResult]) -> Option<&serde_json::Value> {
    results
        .iter()
        .rev()
        .find(|r| {
            r.data.get("imageCount").is_some()
                || r.data.get("images").is_some()
                || r.data.get("download_url").is_some()
                || r.data.get("url").is_some()
        })
        .map(|r| &r.data)
}

pub(crate) fn mark_task_completed(
    state: &web::Data<AppState>,
    ctx: &ImageGenContext,
    result_json: &serde_json::Value,
) {
    let storage = state.storage.lock_recover();
    let _ = storage.update_task_status(&ctx.request_id, TaskStatus::Completed, None, None);
    let _ = storage.update_task_result_data(&ctx.request_id, &result_json.to_string());
}

pub(crate) fn mark_task_failed(state: &web::Data<AppState>, ctx: &ImageGenContext, message: &str) {
    let storage = state.storage.lock_recover();
    let _ = storage.update_task_status(&ctx.request_id, TaskStatus::Failed, None, Some(message));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_response() -> ImageApiResponse {
        ImageApiResponse {
            created: 1_620_000_000,
            images: vec!["abc".to_string()],
            partial_images: vec![PartialImageEvent {
                index: 0,
                b64_json: "partial".to_string(),
                output_format: "png".to_string(),
            }],
            output_format: "png".to_string(),
            quality: "high".to_string(),
            size: "1024x1024".to_string(),
            background: Some("transparent".to_string()),
        }
    }

    #[test]
    fn json_response_uses_openai_image_shape() {
        let body = sample_response().to_json_value();

        assert_eq!(body["created"], 1_620_000_000);
        assert_eq!(body["data"][0]["b64_json"], "abc");
        assert_eq!(body["background"], "transparent");
        assert!(body["data"][0].get("url").is_none());
    }

    #[test]
    fn sse_response_uses_openai_event_shape() {
        let body = sample_response().to_sse_body(ImageEndpoint::Generation);

        assert!(body.starts_with("event: image_generation.partial_image\n"));
        assert!(body.contains("\"type\":\"image_generation.partial_image\""));
        assert!(body.contains("event: image_generation.completed\n"));
        assert!(body.contains("\"type\":\"image_generation.completed\""));
        assert!(body.contains("\"created_at\":1620000000"));
        assert!(body.ends_with("\n\n"));
    }
}
