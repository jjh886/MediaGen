use actix_web::HttpResponse;
use mediagen_browser_gateway::{BrowserSessionCreateRequest, BrowserSessionPurpose};
use serde_json::{json, Value};

use crate::account_browser_profile::add_browser_profile_metadata;
use crate::ServerState;

/// 会话创建前的通用补齐逻辑：只处理账号、代理、浏览器画像这些跨平台原子数据。
pub(crate) fn prepare_browser_session_create_request(
    state: &ServerState,
    request: &mut BrowserSessionCreateRequest,
) -> Result<(), HttpResponse> {
    apply_relogin_defaults(state, request)?;
    apply_saved_proxy(state, request)
}

fn apply_relogin_defaults(
    state: &ServerState,
    request: &mut BrowserSessionCreateRequest,
) -> Result<(), HttpResponse> {
    if request.purpose != BrowserSessionPurpose::ReloginAccount {
        return Ok(());
    }
    let Some(account_id) = request.account_id.as_deref().map(str::trim) else {
        return Ok(());
    };
    let Some(account) = state.store.account_by_id(account_id) else {
        return Err(HttpResponse::BadRequest().json(json!({
            "ok": false,
            "error": "账号不存在，请刷新后重试。"
        })));
    };
    apply_account_profile_dir(request, &account.profile_dir);
    apply_account_proxy(request, &account.proxy_id, &account.proxy_url);
    add_missing_browser_profile(request, &account.browser_os, &account.browser_version);
    Ok(())
}

fn apply_account_profile_dir(request: &mut BrowserSessionCreateRequest, profile_dir: &str) {
    if request
        .profile_dir
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
        && !profile_dir.trim().is_empty()
    {
        request.profile_dir = Some(profile_dir.to_string());
    }
}

fn apply_account_proxy(request: &mut BrowserSessionCreateRequest, proxy_id: &str, proxy_url: &str) {
    if has_proxy_override(request) {
        return;
    }
    if !proxy_id.trim().is_empty() {
        request.proxy_id = Some(proxy_id.to_string());
    } else if !proxy_url.trim().is_empty() {
        request.metadata.insert(
            "proxy_server".to_string(),
            Value::String(proxy_url.to_string()),
        );
    }
}

fn has_proxy_override(request: &BrowserSessionCreateRequest) -> bool {
    explicit_direct_proxy(request)
        || request
            .proxy_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
        || request
            .metadata
            .get("proxy_server")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
}

fn explicit_direct_proxy(request: &BrowserSessionCreateRequest) -> bool {
    request
        .metadata
        .get("proxy_mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| value == "direct")
}

fn add_missing_browser_profile(
    request: &mut BrowserSessionCreateRequest,
    browser_os: &str,
    browser_version: &str,
) {
    let has_profile = request
        .metadata
        .contains_key(mediagen_browser_gateway::BROWSER_OS_METADATA_KEY)
        || request
            .metadata
            .contains_key(mediagen_browser_gateway::BROWSER_VERSION_METADATA_KEY);
    if !has_profile {
        add_browser_profile_metadata(&mut request.metadata, browser_os, browser_version);
    }
}

fn apply_saved_proxy(
    state: &ServerState,
    request: &mut BrowserSessionCreateRequest,
) -> Result<(), HttpResponse> {
    let Some(proxy_id) = request
        .proxy_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let Some(proxy) = state.store.proxy_by_id(proxy_id) else {
        return Err(HttpResponse::BadRequest().json(json!({
            "ok": false,
            "error": "代理不存在，请刷新后重新选择。"
        })));
    };
    if !proxy.enabled {
        return Err(HttpResponse::BadRequest().json(json!({
            "ok": false,
            "error": "代理已停用，请换一个代理。"
        })));
    }
    request
        .metadata
        .insert("proxy_server".to_string(), Value::String(proxy.server));
    if !proxy.username.trim().is_empty() {
        request
            .metadata
            .insert("proxy_username".to_string(), Value::String(proxy.username));
    }
    if !proxy.password.is_empty() {
        request
            .metadata
            .insert("proxy_password".to_string(), Value::String(proxy.password));
    }
    Ok(())
}
