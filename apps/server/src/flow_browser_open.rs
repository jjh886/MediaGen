use std::sync::Arc;

use mediagen_browser_gateway::{
    BrowserGateway, BrowserNavigateRequest, BrowserSessionCreateRequest, BrowserSessionPurpose,
    BrowserSessionResponse,
};
use mediagen_core::PluginManifest;
use serde_json::{json, Value};

use crate::account_browser_profile::browser_metadata_from_account;
use crate::flow_browser_lifecycle::is_persistent_browser;
use crate::store::AccountRecord;

/// 打开任务浏览器：同 profile 首次启动一个进程，后续并发任务只新增独立 Tab。
pub(crate) async fn open_browser(
    browser_gateway: Arc<BrowserGateway>,
    params: &Value,
    manifest: &PluginManifest,
    account: &AccountRecord,
    task_id: Option<&str>,
) -> Result<Value, String> {
    let start_url = start_url(params, manifest);
    let profile_dir =
        text_param(params, "profile_dir").unwrap_or_else(|| account.profile_dir.clone());
    let request = create_request(params, manifest, account, task_id, &start_url, &profile_dir);

    // 同一 profile 的首次启动必须串行，等待者随后会进入“创建新 Tab”分支。
    let profile_lock = browser_gateway
        .profile_open_lock(&profile_dir)
        .map_err(|error| error.to_string())?;
    let _profile_guard = profile_lock.lock().await;
    if let Some(result) = reuse_idle_session(
        &browser_gateway,
        manifest,
        account,
        task_id,
        &profile_dir,
        &start_url,
    )
    .await?
    {
        return Ok(result);
    }
    open_tab_or_process(&browser_gateway, request).await
}

async fn reuse_idle_session(
    browser_gateway: &BrowserGateway,
    manifest: &PluginManifest,
    account: &AccountRecord,
    task_id: Option<&str>,
    profile_dir: &str,
    start_url: &str,
) -> Result<Option<Value>, String> {
    if !is_persistent_browser(manifest) {
        return Ok(None);
    }
    let claimed = browser_gateway
        .claim_idle_profile_session(profile_dir, &account.id, task_id, start_url)
        .map_err(|error| error.to_string())?;
    let Some(response) = claimed else {
        return Ok(None);
    };
    let session_id = response.session.id.clone();
    let navigate = browser_gateway
        .navigate_session(
            &session_id,
            BrowserNavigateRequest {
                url: start_url.to_string(),
            },
        )
        .await;
    if navigate.is_ok() {
        return Ok(Some(session_value(response, true, false)));
    }
    // 常驻进程已失效时先回收旧会话，再按正常路径启动新进程。
    let _ = browser_gateway.cancel_session(&session_id).await;
    Ok(None)
}

async fn open_tab_or_process(
    browser_gateway: &BrowserGateway,
    request: BrowserSessionCreateRequest,
) -> Result<Value, String> {
    if let Some(response) = browser_gateway
        .create_profile_tab_session(request.clone())
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(session_value(response, true, true));
    }
    let response = browser_gateway
        .create_session(request)
        .await
        .map_err(|error| error.to_string())?;
    Ok(session_value(response, false, false))
}

fn create_request(
    params: &Value,
    manifest: &PluginManifest,
    account: &AccountRecord,
    task_id: Option<&str>,
    start_url: &str,
    profile_dir: &str,
) -> BrowserSessionCreateRequest {
    BrowserSessionCreateRequest {
        purpose: BrowserSessionPurpose::TaskTakeover,
        platform: manifest.platform_id.clone(),
        account_id: Some(account.id.clone()),
        task_id: task_id.map(str::to_string),
        start_url: start_url.to_string(),
        profile_dir: Some(profile_dir.to_string()),
        headless: Some(bool_param(params, "headless").unwrap_or(true)),
        metadata: browser_metadata_from_account(account),
        ..BrowserSessionCreateRequest::default()
    }
}

fn session_value(response: BrowserSessionResponse, reused: bool, new_tab: bool) -> Value {
    json!({
        "session_id": response.session.id,
        "profile_dir": response.session.profile_dir,
        "url": response.session.url,
        "reused": reused,
        "new_tab": new_tab,
    })
}

fn start_url(params: &Value, manifest: &PluginManifest) -> String {
    text_param(params, "restore_url")
        .or_else(|| text_param(params, "url"))
        .unwrap_or_else(|| manifest.workspace_url.clone())
}

fn text_param(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn bool_param(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(Value::as_bool)
}
