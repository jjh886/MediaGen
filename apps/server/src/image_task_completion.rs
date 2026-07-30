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
    state
        .store
        .require_task_verification(task_id, message.clone());
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
        message,
        Some(task_id.to_string()),
    );
    Err((
        409,
        "verification_required",
        format!("需要人工接管浏览器会话: {}", response.session.id),
    ))
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
