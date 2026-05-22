//! Cookie 写入与注入：profile SQLite 预写（macOS/Linux）+ CDP 注入（Windows）

use std::path::PathBuf;
use std::sync::Arc;

use headless_chrome::protocol::cdp::Network;

use crate::error::AppError;

/// 准备 profile 目录（清理锁文件 + 修复 exit_type）
pub(crate) fn prepare_user_data_dir(profile_dir_str: &str) -> Option<PathBuf> {
    if profile_dir_str.is_empty() {
        return None;
    }
    let dir = PathBuf::from(profile_dir_str);
    std::fs::create_dir_all(&dir).ok();
    for lock in &["SingletonLock", "SingletonSocket", "SingletonCookie"] {
        let _ = std::fs::remove_file(dir.join(lock));
    }
    crate::browser_manager::fix_chrome_exit_type_pub(&dir);
    Some(dir)
}

/// 在启动 Chrome 之前，把 cookies 直接写入 profile 的 SQLite 数据库。
/// Chrome 启动时会从 SQLite 加载 cookies，跟正常用户行为一致，不会触发反爬检测。
///
/// Chrome cookies 表规则：`value` 非空时用明文，否则解密 `encrypted_value`。
/// 我们写明文 `value`，`encrypted_value` 设为空 BLOB。
///
/// 关键：必须保留 CDP dump 出来的 `sameSite/sourcePort/sourceScheme/partitionKey`，
/// 否则 `cookies_unique_index(host_key, top_frame_site_key, has_cross_site_ancestor,
/// name, path, source_scheme, source_port)` 不命中，INSERT OR REPLACE 退化成 INSERT，
/// 与 Chrome 自己写入的"真版" cookie 形成字段错位副本，触发 Google 这类严格校验失败。
///
/// 注意：仅在 macOS/Linux 上有效。Windows Chrome 使用 DPAPI 加密，不读明文 value。
pub(crate) fn write_cookies_to_profile(profile_dir: &str, cookies_json: &str) -> usize {
    let default_dir = std::path::Path::new(profile_dir).join("Default");
    std::fs::create_dir_all(&default_dir).ok();
    let cookies_db = default_dir.join("Cookies");

    let cookies = match serde_json::from_str::<Vec<serde_json::Value>>(cookies_json) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let conn = match rusqlite::Connection::open(&cookies_db) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "打开 Cookies DB 失败");
            return 0;
        }
    };

    // 确保表存在（新 profile 首次写入时需要建表）
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cookies(
            creation_utc INTEGER NOT NULL,
            host_key TEXT NOT NULL,
            top_frame_site_key TEXT NOT NULL,
            name TEXT NOT NULL,
            value TEXT NOT NULL,
            encrypted_value BLOB NOT NULL,
            path TEXT NOT NULL,
            expires_utc INTEGER NOT NULL,
            is_secure INTEGER NOT NULL,
            is_httponly INTEGER NOT NULL,
            last_access_utc INTEGER NOT NULL,
            has_expires INTEGER NOT NULL,
            is_persistent INTEGER NOT NULL,
            priority INTEGER NOT NULL,
            samesite INTEGER NOT NULL,
            source_scheme INTEGER NOT NULL,
            source_port INTEGER NOT NULL,
            last_update_utc INTEGER NOT NULL,
            source_type INTEGER NOT NULL,
            has_cross_site_ancestor INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS cookies_unique_index
            ON cookies(host_key, top_frame_site_key, has_cross_site_ancestor, name, path, source_scheme, source_port);"
    );

    let now_chrome = chrome_time_now();
    let mut count = 0usize;

    for c in &cookies {
        let name = match c["name"].as_str() {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let value = c["value"].as_str().unwrap_or("");
        let domain = c["domain"].as_str().unwrap_or("");
        let path = c["path"].as_str().unwrap_or("/");
        let is_secure = c["secure"].as_bool().unwrap_or(false) as i32;
        let is_httponly = c["httpOnly"]
            .as_bool()
            .or_else(|| c["http_only"].as_bool())
            .unwrap_or(false) as i32;
        let expires = c["expires"]
            .as_f64()
            .map(|e| (e * 1_000_000.0) as i64 + 11_644_473_600_000_000)
            .unwrap_or(0);
        let has_expires = if expires > 0 { 1 } else { 0 };
        let source_scheme = parse_source_scheme(c, is_secure);
        let source_port = parse_source_port(c, is_secure);
        let samesite = parse_same_site(c);
        let (top_frame_site_key, has_cross_site_ancestor) = parse_partition_key(c);

        let result = conn.execute(
            "INSERT OR REPLACE INTO cookies (
                creation_utc, host_key, top_frame_site_key, name, value, encrypted_value,
                path, expires_utc, is_secure, is_httponly, last_access_utc,
                has_expires, is_persistent, priority, samesite, source_scheme,
                source_port, last_update_utc, source_type, has_cross_site_ancestor
            ) VALUES (?1, ?2, ?3, ?4, ?5, X'', ?6, ?7, ?8, ?9, ?10, ?11, ?11, 1, ?12, ?13, ?14, ?15, 0, ?16)",
            rusqlite::params![
                now_chrome + count as i64,  // creation_utc（每条递增避免冲突）
                domain,
                top_frame_site_key,
                name,
                value,
                path,
                expires,
                is_secure,
                is_httponly,
                now_chrome,  // last_access_utc
                has_expires, // 同时用于 ?11 (has_expires + is_persistent)
                samesite,
                source_scheme,
                source_port,
                now_chrome,  // last_update_utc
                has_cross_site_ancestor,
            ],
        );

        match result {
            Ok(_) => count += 1,
            Err(e) => tracing::debug!(cookie = name, error = %e, "写入 cookie 失败"),
        }
    }

    tracing::info!(count, "cookies 已写入 profile SQLite（Chrome 启动前）");
    count
}

/// 解析 CDP `sourceScheme` → Chrome enum：Unset=0, NonSecure=1, Secure=2。
/// 缺失时按 `is_secure` 推断（保留旧逻辑兜底）。
pub(crate) fn parse_source_scheme(c: &serde_json::Value, is_secure: i32) -> i32 {
    match c.get("sourceScheme").and_then(|v| v.as_str()) {
        Some("Secure") => 2,
        Some("NonSecure") => 1,
        Some("Unset") => 0,
        _ => {
            if is_secure == 1 {
                2
            } else {
                1
            }
        }
    }
}

/// 解析 CDP `sourcePort`。CDP 协议规定端口未知时返回 -1，必须如实保留：
/// 改成 443/80 反而和 Chrome 自己写入的 -1 行不上 unique index，制造副本。
pub(crate) fn parse_source_port(c: &serde_json::Value, is_secure: i32) -> i32 {
    match c.get("sourcePort").and_then(|v| v.as_i64()) {
        Some(p) => p as i32,
        // 字段缺失（极旧 Chrome dump）才退到默认端口
        None => {
            if is_secure == 1 {
                443
            } else {
                80
            }
        }
    }
}

/// 解析 CDP `sameSite` → Chrome enum：Unspecified=-1, None=0, Lax=1, Strict=2。
/// 大多数现代 cookie 是 Lax；硬编码 0 (None) 会让 Chrome 在跨站时拒绝下发，校验失败。
pub(crate) fn parse_same_site(c: &serde_json::Value) -> i32 {
    match c.get("sameSite").and_then(|v| v.as_str()) {
        Some("Strict") => 2,
        Some("Lax") => 1,
        Some("None") => 0,
        _ => -1, // Unspecified
    }
}

/// 解析 CDP `partitionKey` → (top_frame_site_key, has_cross_site_ancestor)。
/// 新版 CDP 返回对象 `{topLevelSite, hasCrossSiteAncestor}`；旧版可能返回字符串；
/// 缺失时回退到非分区状态 `('', 0)`，与 Chrome 默认一致。
pub(crate) fn parse_partition_key(c: &serde_json::Value) -> (String, i32) {
    match c.get("partitionKey") {
        Some(serde_json::Value::Object(obj)) => {
            let site = obj
                .get("topLevelSite")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let cross = obj
                .get("hasCrossSiteAncestor")
                .and_then(|v| v.as_bool())
                .unwrap_or(false) as i32;
            (site, cross)
        }
        Some(serde_json::Value::String(s)) => (s.clone(), 0),
        _ => (String::new(), 0),
    }
}

/// Chrome 时间戳：从 1601-01-01 开始的微秒数
fn chrome_time_now() -> i64 {
    let unix_micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as i64;
    // Unix epoch (1970) 到 Chrome epoch (1601) 的差值：11644473600 秒
    unix_micros + 11_644_473_600_000_000
}

/// Windows: Chrome 启动后通过 CDP Network.setCookie 注入 cookies
/// 绕过 Windows Chrome 的 DPAPI 加密限制（SQLite 明文 value 不被读取）
pub(crate) async fn inject_cookies_via_cdp(
    tab: &Arc<headless_chrome::Tab>,
    cookies_json: &str,
) -> usize {
    let cookies: Vec<serde_json::Value> = match serde_json::from_str(cookies_json) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let cookie_params: Vec<Network::CookieParam> = cookies
        .iter()
        .filter_map(|c| Some(json_to_cookie_param(c)?))
        .collect();

    let count = cookie_params.len();
    let tab = tab.clone();

    match tokio::task::spawn_blocking(move || {
        tab.set_cookies(cookie_params)
            .map_err(|e| AppError::BrowserError(format!("CDP cookie 注入失败: {e}")))
    })
    .await
    {
        Ok(Ok(())) => {
            tracing::info!(count, "cookies 已通过 CDP 注入");
            count
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "CDP cookie 注入失败");
            0
        }
        Err(e) => {
            tracing::warn!(error = %e, "CDP cookie 注入任务失败");
            0
        }
    }
}

/// 把 dump 出来的 JSON cookie 转成 CDP `Network.CookieParam`，
/// 完整保留 sameSite/sourceScheme/sourcePort/partitionKey/priority。
/// 与 [`write_cookies_to_profile`] 的字段语义保持一致，避免 SQLite 与 CDP 两条路径产生分歧。
pub(crate) fn json_to_cookie_param(c: &serde_json::Value) -> Option<Network::CookieParam> {
    use headless_chrome::protocol::cdp::Network::{
        CookiePartitionKey, CookiePriority, CookieSameSite, CookieSourceScheme,
    };

    let name = c
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|n| !n.is_empty())?;
    let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
    let domain = c.get("domain").and_then(|v| v.as_str()).map(String::from);
    let secure = c.get("secure").and_then(|v| v.as_bool());
    // CDP 需要 url 来设置跨域 cookie，从 domain 推导
    let url = domain.as_ref().map(|d| {
        let d = d.trim_start_matches('.');
        let scheme = if secure.unwrap_or(false) {
            "https"
        } else {
            "http"
        };
        format!("{scheme}://{d}/")
    });

    let same_site = match c.get("sameSite").and_then(|v| v.as_str()) {
        Some("Strict") => Some(CookieSameSite::Strict),
        Some("Lax") => Some(CookieSameSite::Lax),
        Some("None") => Some(CookieSameSite::None),
        _ => None,
    };
    let source_scheme = match c.get("sourceScheme").and_then(|v| v.as_str()) {
        Some("Secure") => Some(CookieSourceScheme::Secure),
        Some("NonSecure") => Some(CookieSourceScheme::NonSecure),
        Some("Unset") => Some(CookieSourceScheme::Unset),
        _ => None,
    };
    // CDP 协议规定 -1 表示端口未知；headless_chrome 把字段绑定为 u32，
    // 无法表示 -1，遇到这种值传 None，让 Chrome 按 URL 推断。
    let source_port = c.get("sourcePort").and_then(|v| v.as_i64()).and_then(|p| {
        if p > 0 {
            Some(p as u32)
        } else {
            None
        }
    });
    let priority = match c.get("priority").and_then(|v| v.as_str()) {
        Some("Low") => Some(CookiePriority::Low),
        Some("Medium") => Some(CookiePriority::Medium),
        Some("High") => Some(CookiePriority::High),
        _ => None,
    };
    let partition_key = c.get("partitionKey").and_then(|pk| match pk {
        serde_json::Value::Object(obj) => Some(CookiePartitionKey {
            top_level_site: obj
                .get("topLevelSite")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            has_cross_site_ancestor: obj
                .get("hasCrossSiteAncestor")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }),
        serde_json::Value::String(s) => Some(CookiePartitionKey {
            top_level_site: s.clone(),
            has_cross_site_ancestor: false,
        }),
        _ => None,
    });

    Some(Network::CookieParam {
        name: name.to_string(),
        value: value.to_string(),
        url,
        domain,
        path: c.get("path").and_then(|v| v.as_str()).map(String::from),
        secure,
        http_only: c
            .get("httpOnly")
            .and_then(|v| v.as_bool())
            .or_else(|| c.get("http_only").and_then(|v| v.as_bool())),
        same_site,
        expires: c.get("expires").and_then(|v| v.as_f64()),
        priority,
        same_party: c.get("sameParty").and_then(|v| v.as_bool()),
        source_scheme,
        source_port,
        partition_key,
    })
}

#[cfg(test)]
#[path = "cookie_tests.rs"]
mod cookie_tests;
