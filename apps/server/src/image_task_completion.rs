use actix_web::web;
use mediagen_core::PluginManifest;
use serde_json::{json, Value};

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
        headless: Some(true),
        metadata: browser_metadata_from_account(account),
        ..mediagen_browser_gateway::BrowserSessionCreateRequest::default()
    };
    let response = state
        .browser_gateway
        .create_session(request)
        .await
        .map_err(|error| (409, "verification_required", error.to_string()))?;
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
    result: Value,
) -> Result<Value, (u16, &'static str, String)> {
    let images = collect_b64_images(&result, proxy_url)
        .await
        .map_err(|message| {
            state.store.fail_task(task_id, message.clone());
            (502, "image_result_error", message)
        })?;
    state
        .store
        .complete_task(task_id, image_result_metadata(&result));
    Ok(json!({
        "created": chrono::Utc::now().timestamp(),
        "data": images.into_iter().map(|b64| json!({ "b64_json": b64 })).collect::<Vec<_>>(),
    }))
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
