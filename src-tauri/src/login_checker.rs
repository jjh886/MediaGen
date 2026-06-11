//! 登录状态检查（HTTP 请求验证，不启动 Chrome）
//!
//! 从插件 manifest 的 `login_check` 配置读取验证接口，
//! 用 DB 中保存的 cookies 发 HTTP 请求，判断 session 是否仍然有效。
//! 不含任何平台特定逻辑。

use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};

use reqwest::{Response, StatusCode};

use crate::engine::EngineRegistry;
use crate::login_checker_flow::{check_account_flow, persist_account_outcome, AccountCheckOutcome};
use crate::plugin_loader::PluginLoader;
use crate::storage::Storage;
use crate::sync_ext::MutexExt;
use crate::types::{LoginCheck, LoginState, PlatformAccount};

// ── Cookie 处理 ──

#[derive(serde::Deserialize)]
struct CookieEntry {
    name: String,
    value: String,
    domain: Option<String>,
}

/// 从 DB 的 cookies JSON 构造 Cookie header，按域名过滤
fn build_cookie_header(cookies_json: &str, domain_filter: &str) -> String {
    let cookies: Vec<CookieEntry> = match serde_json::from_str(cookies_json) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    cookies
        .iter()
        .filter(|c| {
            c.domain
                .as_deref()
                .map(|d| d.trim_start_matches('.').contains(domain_filter))
                .unwrap_or(true)
        })
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// 按 dot path 取 JSON 字段值（如 "user.email" → json["user"]["email"]）
fn resolve_field(json: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = json;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    match current {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

// ── 验证逻辑 ──

/// 只有“足够明确”的 HTTP 状态才会直接判定账号过期。
/// 403/429/5xx 更常见于风控、限流或服务端临时异常，不能直接把正常账号打成过期。
fn login_expired_reason_for_status(status: StatusCode) -> Option<String> {
    if status.is_redirection() {
        return Some(format!("HTTP {status}，重定向到登录页"));
    }
    if status == StatusCode::UNAUTHORIZED {
        return Some(format!("HTTP {status}，登录凭据无效"));
    }
    None
}

fn is_inconclusive_status(status: StatusCode, config: &LoginCheck) -> bool {
    config.inconclusive_statuses.contains(&status.as_u16())
}

enum StatusCheck {
    Continue,
    Expired(String),
    Inconclusive,
}

/// 用 HTTP 请求验证单个账号的登录态（通用，不含平台逻辑）。
/// 这里故意保守：只有明确的 401/重定向/过期时间才判过期，其它探测异常都归为“不确定”。
async fn check_account_http(account: &PlatformAccount, config: &LoginCheck) -> AccountCheckOutcome {
    let cookie_header = match resolve_cookie_header(account, config) {
        Ok(header) => header,
        Err(reason) => return AccountCheckOutcome::Expired(reason),
    };

    let Some(resp) = send_login_check_request(account, config, &cookie_header).await else {
        return AccountCheckOutcome::Inconclusive("登录检查请求失败".to_string());
    };

    match classify_login_status(account, config, resp.status()) {
        StatusCheck::Continue => {}
        StatusCheck::Expired(reason) => return AccountCheckOutcome::Expired(reason),
        StatusCheck::Inconclusive => {
            return AccountCheckOutcome::Inconclusive(format!("HTTP {}", resp.status()));
        }
    };

    let json = match read_login_check_json(resp).await {
        Some(Ok(json)) => json,
        Some(Err(reason)) => return AccountCheckOutcome::Inconclusive(reason),
        None => return AccountCheckOutcome::Inconclusive("登录检查响应读取失败".to_string()),
    };

    let outcome = classify_login_json(&json, config);
    if !matches!(&outcome, AccountCheckOutcome::Valid(_)) {
        return outcome;
    };

    debug!(
        account_id = %account.id,
        platform = %account.platform,
        "HTTP 登录验证通过"
    );
    outcome
}

fn resolve_cookie_header(account: &PlatformAccount, config: &LoginCheck) -> Result<String, String> {
    if account.cookies.is_empty() || account.cookies == "[]" {
        return Err("无 cookies 数据".into());
    }

    let cookie_header = build_cookie_header(&account.cookies, &config.cookie_domain);
    if cookie_header.is_empty() {
        return Err(format!("cookies 中无匹配域名 '{}'", config.cookie_domain));
    }
    Ok(cookie_header)
}

async fn send_login_check_request(
    account: &PlatformAccount,
    config: &LoginCheck,
    cookie_header: &str,
) -> Option<Response> {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "HTTP 客户端初始化失败（不标记过期）");
            return None;
        }
    };

    match client
        .get(&config.url)
        .header("Cookie", cookie_header)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        )
        .send()
        .await
    {
        Ok(resp) => Some(resp),
        Err(e) => {
            warn!(
                account_id = %account.id,
                error = %e,
                "登录检查请求失败（不标记过期）"
            );
            None
        }
    }
}

fn classify_login_status(
    account: &PlatformAccount,
    config: &LoginCheck,
    status: StatusCode,
) -> StatusCheck {
    if let Some(reason) = login_expired_reason_for_status(status) {
        return StatusCheck::Expired(reason);
    }
    if status.is_success() {
        return StatusCheck::Continue;
    }
    if is_inconclusive_status(status, config) {
        // 平台可把风控、限流、临时服务异常声明为“不确定”，避免后台 HTTP 探测误伤账号。
        warn!(
            account_id = %account.id,
            platform = %account.platform,
            status = %status,
            "登录检查返回非确定性状态（不标记过期）"
        );
        return StatusCheck::Inconclusive;
    }
    StatusCheck::Expired(format!("HTTP {status}"))
}

async fn read_login_check_json(resp: Response) -> Option<Result<serde_json::Value, String>> {
    let body = match resp.text().await {
        Ok(body) => body,
        Err(_) => return None,
    };
    match serde_json::from_str(&body) {
        Ok(json) => Some(Ok(json)),
        Err(_) => Some(Err("响应非 JSON".into())),
    }
}

fn validate_success_field(json: &serde_json::Value, config: &LoginCheck) -> Option<String> {
    if resolve_field(json, &config.success_field).is_some() {
        return None;
    }
    Some(format!("字段 '{}' 为空或不存在", config.success_field))
}

enum ExpiryState {
    Fresh,
    Expired(String),
    Unknown,
}

fn classify_login_json(json: &serde_json::Value, config: &LoginCheck) -> AccountCheckOutcome {
    match session_expiry_state(json, config) {
        ExpiryState::Expired(reason) => return AccountCheckOutcome::Expired(reason),
        ExpiryState::Fresh => return AccountCheckOutcome::Valid(None),
        ExpiryState::Unknown => {}
    }
    if let Some(reason) = validate_success_field(json, config) {
        return AccountCheckOutcome::Inconclusive(reason);
    }
    AccountCheckOutcome::Valid(None)
}

fn session_expiry_state(json: &serde_json::Value, config: &LoginCheck) -> ExpiryState {
    let Some(expires_path) = config.expires_field.as_ref() else {
        return ExpiryState::Unknown;
    };
    let Some(expires_str) = resolve_field(json, expires_path) else {
        return ExpiryState::Unknown;
    };
    let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(&expires_str) else {
        return ExpiryState::Unknown;
    };
    if expires_at >= chrono::Utc::now() {
        return ExpiryState::Fresh;
    }
    ExpiryState::Expired(format!("session 已过期 ({expires_str})"))
}

// ── 批量检查 ──

/// 检查所有 Available 账号的登录状态并更新数据库
pub async fn check_all_accounts(
    storage: &Arc<Mutex<Storage>>,
    plugin_loader: &Arc<PluginLoader>,
    engine_registry: Option<&EngineRegistry>,
) -> (usize, usize) {
    let accounts = {
        let s = storage.lock_recover();
        match s.list_all_accounts() {
            Ok(a) => a,
            Err(e) => {
                warn!(error = %e, "获取账号列表失败");
                return (0, 0);
            }
        }
    };

    let mut checked = 0usize;
    let mut expired = 0usize;

    for account in &accounts {
        if !should_check_login_state(&account.login_state) {
            continue;
        }

        let Some(outcome) =
            check_account_by_manifest(account, plugin_loader, engine_registry).await
        else {
            continue;
        };

        checked += 1;

        if persist_account_outcome(storage, account, outcome) {
            expired += 1;
        }
    }

    if checked > 0 {
        info!(checked, expired, "登录状态检查完成");
    }

    (checked, expired)
}

fn should_check_login_state(state: &LoginState) -> bool {
    matches!(state, LoginState::Available | LoginState::LoginExpired)
}

async fn check_account_by_manifest(
    account: &PlatformAccount,
    plugin_loader: &Arc<PluginLoader>,
    engine_registry: Option<&EngineRegistry>,
) -> Option<AccountCheckOutcome> {
    let manifest = plugin_loader.get_plugin(&account.platform)?;
    if let Some(config) = manifest.login_check.as_ref() {
        return Some(check_account_http(account, config).await);
    }
    let engine = engine_registry?;
    let flow = manifest.flows.get("check_account")?;
    Some(check_account_flow(account, manifest, plugin_loader, flow, engine).await)
}

/// 启动后台定时检查任务。
/// 每轮循环前重新从 settings 读取 `check_interval_minutes`，修改设置后下一轮即生效。
pub fn spawn_periodic_checker(
    storage: Arc<Mutex<Storage>>,
    plugin_loader: Arc<PluginLoader>,
    engine_registry: Arc<EngineRegistry>,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("创建 login_checker runtime 失败");

        rt.block_on(async move {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let (checked, expired) =
                check_all_accounts(&storage, &plugin_loader, Some(engine_registry.as_ref())).await;
            if checked > 0 {
                info!(checked, expired, "启动时登录状态检查完成");
            }

            loop {
                let minutes = read_check_interval_minutes(&storage);
                tokio::time::sleep(std::time::Duration::from_secs(minutes * 60)).await;
                check_all_accounts(&storage, &plugin_loader, Some(engine_registry.as_ref())).await;
            }
        });
    });
}

fn read_check_interval_minutes(storage: &Arc<Mutex<Storage>>) -> u64 {
    let s = storage.lock_recover();
    s.get_setting("check_interval_minutes")
        .ok()
        .flatten()
        .and_then(|v| v.trim_matches('"').parse::<u64>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(30)
}

#[cfg(test)]
#[path = "login_checker_tests.rs"]
mod tests;
