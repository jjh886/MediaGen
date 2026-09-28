use actix_web::HttpResponse;
use mediagen_browser_gateway::{
    BrowserFingerprint, BrowserGatewayError, BrowserSessionCreateRequest, BrowserSessionPurpose,
};
use serde_json::{json, Value};

use crate::account_browser_profile::add_browser_profile_metadata;
use crate::ServerState;

/// 会话创建前的通用补齐逻辑：只处理账号、代理、浏览器画像这些跨平台原子数据。
pub(crate) fn prepare_browser_session_create_request(
    state: &ServerState,
    request: &mut BrowserSessionCreateRequest,
) -> Result<(), HttpResponse> {
    apply_shared_browser_profile(state, request)?;
    apply_relogin_defaults(state, request)?;
    validate_browser_fingerprint(request)?;
    apply_saved_proxy(state, request)
}

fn validate_browser_fingerprint(request: &BrowserSessionCreateRequest) -> Result<(), HttpResponse> {
    let os = request
        .metadata
        .get("browser_os")
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = request
        .metadata
        .get("browser_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    if (os.is_empty() && version.is_empty())
        || BrowserFingerprint::from_values(os, version).is_some()
    {
        return Ok(());
    }
    Err(HttpResponse::BadRequest().json(json!({
        "ok": false,
        "error": "系统指纹的系统或浏览器版本无效。"
    })))
}

fn apply_shared_browser_profile(
    state: &ServerState,
    request: &mut BrowserSessionCreateRequest,
) -> Result<(), HttpResponse> {
    let Some(source_id) = request.reuse_browser_profile_account_id.as_deref() else {
        return Ok(());
    };
    if request.purpose != BrowserSessionPurpose::AddAccount || source_id.trim().is_empty() {
        return Err(HttpResponse::BadRequest().json(json!({
            "ok": false,
            "error": "仅添加账号时可选择复用已有账号浏览器资料。"
        })));
    }
    let Some(source) = state.store.account_by_id(source_id.trim()) else {
        return Err(HttpResponse::BadRequest().json(json!({
            "ok": false,
            "error": "来源账号不存在，请刷新后重试。"
        })));
    };
    let browser_dir = source.effective_browser_profile_dir().trim();
    if browser_dir.is_empty() {
        return Err(HttpResponse::BadRequest().json(json!({
            "ok": false,
            "error": "来源账号没有可复用的浏览器资料。"
        })));
    }
    // 账号凭据仍存新账号独立目录，浏览器的 Cookie 和本地存储才持续共用。
    request.profile_dir = None;
    request.browser_profile_dir = Some(browser_dir.to_string());
    request.proxy_id = None;
    request.metadata.remove("proxy_server");
    request.metadata.remove("proxy_username");
    request.metadata.remove("proxy_password");
    request.metadata.remove("proxy_mode");
    request.metadata.remove("browser_os");
    request.metadata.remove("browser_version");
    apply_account_proxy(request, &source.proxy_id, &source.proxy_url);
    add_missing_browser_profile(request, &source.browser_os, &source.browser_version);
    Ok(())
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
    request.browser_profile_dir = Some(account.effective_browser_profile_dir().to_string());
    apply_account_proxy(request, &account.proxy_id, &account.proxy_url);
    add_missing_browser_profile(request, &account.browser_os, &account.browser_version);
    Ok(())
}

/// 重新登录需要独占原资料目录；先关闭占用它的旧会话，再原样复用其中的登录数据。
pub(crate) async fn release_relogin_profile(
    state: &ServerState,
    request: &BrowserSessionCreateRequest,
) -> Result<usize, BrowserGatewayError> {
    if request.purpose != BrowserSessionPurpose::ReloginAccount {
        return Ok(0);
    }
    let profile_dir = request
        .browser_profile_dir
        .as_deref()
        .unwrap_or_default()
        .trim();
    state
        .browser_gateway
        .cancel_active_profile_sessions(profile_dir)
        .await
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
