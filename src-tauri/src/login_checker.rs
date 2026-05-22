//! 登录状态检查（HTTP 请求验证，不启动 Chrome）
//!
//! 从插件 manifest 的 `login_check` 配置读取验证接口，
//! 用 DB 中保存的 cookies 发 HTTP 请求，判断 session 是否仍然有效。
//! 不含任何平台特定逻辑。

use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};

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

/// 用 HTTP 请求验证单个账号的登录态（通用，不含平台逻辑）
///
/// 返回 `Some(reason)` 表示过期，`None` 表示仍然有效
async fn check_account_http(account: &PlatformAccount, config: &LoginCheck) -> Option<String> {
    if account.cookies.is_empty() || account.cookies == "[]" {
        return Some("无 cookies 数据".into());
    }

    let cookie_header = build_cookie_header(&account.cookies, &config.cookie_domain);
    if cookie_header.is_empty() {
        return Some(format!("cookies 中无匹配域名 '{}'", config.cookie_domain));
    }

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

    let resp = match client
        .get(&config.url)
        .header("Cookie", &cookie_header)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        )
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!(
                account_id = %account.id,
                error = %e,
                "登录检查请求失败（不标记过期）"
            );
            return None;
        }
    };

    let status = resp.status();

    if status.is_redirection() {
        return Some(format!("HTTP {status}，重定向到登录页"));
    }
    if !status.is_success() {
        return Some(format!("HTTP {status}"));
    }

    let body = match resp.text().await {
        Ok(b) => b,
        Err(_) => return None,
    };

    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(j) => j,
        Err(_) => return Some("响应非 JSON".into()),
    };

    if resolve_field(&json, &config.success_field).is_none() {
        return Some(format!("字段 '{}' 为空或不存在", config.success_field));
    }

    // 可选：检查过期时间字段
    if let Some(ref expires_path) = config.expires_field {
        if let Some(expires_str) = resolve_field(&json, expires_path) {
            if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(&expires_str) {
                if expires_at < chrono::Utc::now() {
                    return Some(format!("session 已过期 ({})", expires_str));
                }
            }
        }
    }

    debug!(
        account_id = %account.id,
        platform = %account.platform,
        "HTTP 登录验证通过"
    );
    None
}

// ── 批量检查 ──

/// 检查所有 Available 账号的登录状态并更新数据库
pub async fn check_all_accounts(
    storage: &Arc<Mutex<Storage>>,
    plugin_loader: &Arc<PluginLoader>,
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
        if account.login_state != LoginState::Available {
            continue;
        }

        // 从 manifest 读取 login_check 配置
        let config = plugin_loader
            .get_plugin(&account.platform)
            .and_then(|m| m.login_check.as_ref());

        let config = match config {
            Some(c) => c,
            None => continue, // 无 login_check 配置，跳过
        };

        checked += 1;

        let result = check_account_http(account, config).await;

        if let Some(reason) = result {
            info!(
                account_id = %account.id,
                platform = %account.platform,
                name = %account.name,
                reason = %reason,
                "账号登录已过期"
            );

            let s = storage.lock_recover();
            if let Err(e) = s.update_login_state(&account.id, LoginState::LoginExpired, &reason) {
                warn!(error = %e, "更新登录状态失败");
            }
            expired += 1;
        } else {
            let s = storage.lock_recover();
            if let Err(e) = s.update_last_check_at(&account.id) {
                warn!(error = %e, account_id = %account.id, "更新 last_check_at 失败");
            }
        }
    }

    if checked > 0 {
        info!(checked, expired, "登录状态检查完成");
    }

    (checked, expired)
}

/// 启动后台定时检查任务。
/// 每轮循环前重新从 settings 读取 `check_interval_minutes`，修改设置后下一轮即生效。
pub fn spawn_periodic_checker(storage: Arc<Mutex<Storage>>, plugin_loader: Arc<PluginLoader>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("创建 login_checker runtime 失败");

        rt.block_on(async move {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let (checked, expired) = check_all_accounts(&storage, &plugin_loader).await;
            if checked > 0 {
                info!(checked, expired, "启动时登录状态检查完成");
            }

            loop {
                let minutes = read_check_interval_minutes(&storage);
                tokio::time::sleep(std::time::Duration::from_secs(minutes * 60)).await;
                check_all_accounts(&storage, &plugin_loader).await;
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
