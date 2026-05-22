use std::collections::HashMap;

use serde_json::Value;

use crate::process_ext::NoWindowExt;

pub(crate) fn proxy_url_for_request(url: &str, params: &Value) -> Option<String> {
    if let Some(proxy_url) = params["proxy_url"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(normalize_proxy_url)
    {
        return Some(proxy_url);
    }
    let proxy = params["proxy"].as_str().unwrap_or("");
    let use_system_proxy = params["use_system_proxy"].as_bool().unwrap_or(false);
    if proxy != "system" && !use_system_proxy {
        return None;
    }
    let endpoint = reqwest::Url::parse(url).ok()?;
    if should_bypass_proxy(endpoint.host_str().unwrap_or_default()) {
        return None;
    }
    env_proxy_url(endpoint.scheme()).or_else(system_proxy_url)
}

fn env_proxy_url(scheme: &str) -> Option<String> {
    let keys: &[&str] = if scheme == "https" {
        &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    keys.iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|value| normalize_proxy_url(&value))
}

fn system_proxy_url() -> Option<String> {
    if cfg!(target_os = "windows") {
        return windows_system_proxy_url();
    }
    if cfg!(target_os = "macos") {
        return mac_system_proxy_url();
    }
    None
}

fn windows_system_proxy_url() -> Option<String> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        ])
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let enabled = registry_value(&stdout, "ProxyEnable")
        .and_then(|value| u32::from_str_radix(value.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    if enabled == 0 {
        return None;
    }
    registry_value(&stdout, "ProxyServer").and_then(proxy_url_from_windows_proxy_server)
}

fn registry_value<'a>(stdout: &'a str, name: &str) -> Option<&'a str> {
    stdout
        .lines()
        .find(|line| line.contains(name))
        .and_then(|line| line.split_whitespace().last())
}

fn proxy_url_from_windows_proxy_server(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    if !text.contains('=') {
        return normalize_proxy_url(text);
    }
    let mut http = None;
    let mut https = None;
    for part in text.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "http" => http = normalize_proxy_url(value),
            "https" => https = normalize_proxy_url(value),
            _ => {}
        }
    }
    https.or(http)
}

fn mac_system_proxy_url() -> Option<String> {
    let output = std::process::Command::new("scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once(':') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    if map.get("HTTPSEnable").map(String::as_str) == Some("1") {
        if let (Some(host), Some(port)) = (map.get("HTTPSProxy"), map.get("HTTPSPort")) {
            return normalize_proxy_url(&format!("{host}:{port}"));
        }
    }
    if map.get("HTTPEnable").map(String::as_str) == Some("1") {
        if let (Some(host), Some(port)) = (map.get("HTTPProxy"), map.get("HTTPPort")) {
            return normalize_proxy_url(&format!("{host}:{port}"));
        }
    }
    None
}

fn normalize_proxy_url(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() || text.to_ascii_lowercase().starts_with("socks") {
        return None;
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        Some(text.to_string())
    } else {
        Some(format!("http://{text}"))
    }
}

fn should_bypass_proxy(hostname: &str) -> bool {
    let hostname = hostname.to_ascii_lowercase();
    let no_proxy = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();
    no_proxy
        .split(',')
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .any(|item| item == "*" || hostname == item || hostname.ends_with(&format!(".{item}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_proxy_parser_prefers_https_entry() {
        assert_eq!(
            proxy_url_from_windows_proxy_server("http=127.0.0.1:7890;https=127.0.0.1:7891"),
            Some("http://127.0.0.1:7891".to_string())
        );
        assert_eq!(
            proxy_url_from_windows_proxy_server("127.0.0.1:7890"),
            Some("http://127.0.0.1:7890".to_string())
        );
    }

    #[test]
    fn registry_parser_requires_proxy_enabled() {
        let output = r#"
ProxyEnable    REG_DWORD    0x1
ProxyServer    REG_SZ       http=127.0.0.1:7890;https=127.0.0.1:7891
"#;
        let server = registry_value(output, "ProxyServer").unwrap();
        assert_eq!(
            proxy_url_from_windows_proxy_server(server),
            Some("http://127.0.0.1:7891".to_string())
        );
    }
}
