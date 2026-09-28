use actix_web::web;
use mediagen_core::PluginManifest;
use serde_json::Value;

use crate::account_browser_profile::browser_metadata_from_account;
use crate::image_api_response::image_result_metadata;
use crate::image_assets::collect_b64_images;
use crate::store::AccountRecord;
use crate::ServerState;

pub(crate) async fn create_verification_session(
    state: &web::Data<ServerState>,
    manifest: &PluginManifest,
    account: &AccountRecord,
    task_id: &str,
    message: String,
) -> Result<Value, (u16, &'static str, String)> {
    let safe_message = safe_verification_message(&message);
    state
        .store
        .require_task_verification(task_id, safe_message.clone());
    let request = mediagen_browser_gateway::BrowserSessionCreateRequest {
        purpose: mediagen_browser_gateway::BrowserSessionPurpose::HumanVerification,
        platform: manifest.platform_id.clone(),
        account_id: Some(account.id.clone()),
        task_id: Some(task_id.to_string()),
        start_url: manifest.workspace_url.clone(),
        profile_dir: Some(account.profile_dir.clone()),
        browser_profile_dir: Some(account.effective_browser_profile_dir().to_string()),
        headless: Some(true),
        metadata: browser_metadata_from_account(account),
        ..mediagen_browser_gateway::BrowserSessionCreateRequest::default()
    };
    let profile_lock = state
        .browser_gateway
        .profile_open_lock(account.effective_browser_profile_dir())
        .map_err(|error| (409, "verification_required", error.to_string()))?;
    let _guard = profile_lock.lock().await;
    let response = match state
        .browser_gateway
        .create_profile_tab_session(request.clone())
        .await
    {
        Ok(Some(response)) => Ok(response),
        Ok(None) => state.browser_gateway.create_session(request).await,
        Err(error) => Err(error),
    }
    .map_err(|error| (409, "verification_required", error.to_string()))?;
    if let Some(detection) = manifest.extra.get("login_detection") {
        if detection.get("context_file").is_some() {
            let references = crate::flow_templates::collect_script_files(detection);
            if !references.is_empty() {
                // 插件根据持久化验证上下文呈现挑战；平台规则由脚本处理。
                let _ = crate::account_login::run_login_script_value(
                    state,
                    &response.session,
                    &references,
                    true,
                )
                .await;
            }
        }
    }
    state.store.add_audit_log(
        "account",
        "warn",
        "任务需要人工验证".to_string(),
        safe_message,
        Some(task_id.to_string()),
    );
    Err((
        409,
        "verification_required",
        format!("需要人工接管浏览器会话: {}", response.session.id),
    ))
}

/// 风控原文可能包含平台内部令牌，任务与审计记录只保存稳定、可识别的信息。
fn safe_verification_message(message: &str) -> String {
    if message.contains("710022004") {
        "平台要求人工验证（710022004）".to_string()
    } else {
        "平台要求人工验证".to_string()
    }
}

pub(crate) async fn complete_image_task(
    state: &web::Data<ServerState>,
    task_id: &str,
    proxy_url: &str,
    body: &crate::image_request::OpenAiImageRequest,
    mut result: Value,
) -> Result<Value, (u16, &'static str, String)> {
    let images = collect_b64_images(&result, proxy_url)
        .await
        .map_err(|message| {
            state.store.fail_task(task_id, message.clone());
            (502, "image_result_error", message)
        })?;
    let format = body.output_format.clone().unwrap_or_else(|| "png".into());
    for (field, requested) in [("quality", &body.quality), ("background", &body.background)] {
        if result[field].is_null() {
            result[field] = serde_json::json!(requested.as_deref().unwrap_or("auto"));
        }
    }
    let compression = body.output_compression;
    let dimensions = body
        .output_dimensions()
        .map_err(|message| (400, "invalid_request_error", message))?;
    let metadata = image_result_metadata(&result);
    let mut response = tokio::task::spawn_blocking(move || {
        crate::image_output::encode(images, result, &format, compression, dimensions)
    })
    .await
    .map_err(|_| (502, "image_result_error", "图片编码任务失败".into()))?
    .map_err(|message| {
        state.store.fail_task(task_id, message.clone());
        (502, "image_result_error", message)
    })?;
    crate::image_output::apply_webp_compression(&mut response, compression)
        .await
        .map_err(|message| {
            state.store.fail_task(task_id, message.clone());
            (502, "image_result_error", message)
        })?;
    if state.store.generation_job(task_id, None).is_some() {
        crate::generation_queue::result::save(state, task_id, &response)
            .await
            .map_err(|_| {
                (
                    500,
                    "RESULT_STORAGE_ERROR",
                    "图片已生成但本地保存失败，请勿重复生成".to_string(),
                )
            })?;
    }
    state.store.complete_task(task_id, metadata);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_message_drops_platform_private_details() {
        let raw = "710022004 decision.detail=private-platform-token";

        let message = safe_verification_message(raw);

        assert_eq!(message, "平台要求人工验证（710022004）");
        assert!(!message.contains("private-platform-token"));
    }
}
