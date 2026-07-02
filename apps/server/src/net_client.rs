pub fn http_client(proxy_url: &str) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy) = normalize_proxy_url(proxy_url) {
        builder = builder.proxy(
            reqwest::Proxy::all(&proxy).map_err(|err| format!("代理地址无效 {proxy}: {err}"))?,
        );
    }
    builder.build().map_err(|err| err.to_string())
}

pub fn normalize_proxy_url(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("socks5://")
        || lower.starts_with("socks5h://")
    {
        Some(text.to_string())
    } else {
        Some(format!("socks5://{text}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_blank_and_explicit_proxy_schemes() {
        assert_eq!(normalize_proxy_url("  "), None);
        assert_eq!(
            normalize_proxy_url("http://127.0.0.1:8080"),
            Some("http://127.0.0.1:8080".to_string())
        );
        assert_eq!(
            normalize_proxy_url("SOCKS5H://user:pass@proxy.test:1080"),
            Some("SOCKS5H://user:pass@proxy.test:1080".to_string())
        );
    }

    #[test]
    fn defaults_missing_scheme_to_socks5() {
        assert_eq!(
            normalize_proxy_url("127.0.0.1:1080"),
            Some("socks5://127.0.0.1:1080".to_string())
        );
    }
}
