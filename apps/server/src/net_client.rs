pub fn http_client(proxy_url: &str) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy) = normalize_proxy_url(proxy_url) {
        builder = builder.proxy(
            reqwest::Proxy::all(&proxy)
                .map_err(|err| format!("代理地址无效: {}", err.without_url()))?,
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

pub fn proxy_url_with_auth(server: &str, username: &str, password: &str) -> String {
    let Some(normalized) = normalize_proxy_url(server) else {
        return String::new();
    };
    let username = username.trim();
    if username.is_empty() {
        return normalized;
    }
    let Ok(mut url) = reqwest::Url::parse(&normalized) else {
        return normalized;
    };
    if url.set_username(username).is_err() {
        return normalized;
    }
    let _ = url.set_password(Some(password));
    url.to_string()
}

pub fn proxy_url_without_auth(value: &str) -> String {
    let Some(normalized) = normalize_proxy_url(value) else {
        return String::new();
    };
    let Some((scheme, rest)) = normalized.split_once("://") else {
        return normalized;
    };
    let host = rest.rsplit_once('@').map(|(_, host)| host).unwrap_or(rest);
    format!("{scheme}://{host}")
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

    #[test]
    fn http_client_accepts_empty_proxy_and_rejects_invalid_proxy() {
        assert!(http_client("").is_ok());
        assert!(http_client("http://").is_err());
    }

    #[test]
    fn proxy_url_with_auth_encodes_credentials() {
        assert_eq!(
            proxy_url_with_auth("socks5h://proxy.test:1080", "u@a", "p:a/ss"),
            "socks5h://u%40a:p%3Aa%2Fss@proxy.test:1080"
        );
        assert_eq!(
            proxy_url_with_auth("proxy.test:1080", "", "pass"),
            "socks5://proxy.test:1080"
        );
    }

    #[test]
    fn proxy_url_without_auth_removes_credentials() {
        assert_eq!(
            proxy_url_without_auth("socks5h://u%40a:p%3Aa%2Fss@proxy.test:1080"),
            "socks5h://proxy.test:1080"
        );
        assert_eq!(
            proxy_url_without_auth("proxy.test:1080"),
            "socks5://proxy.test:1080"
        );
        assert_eq!(proxy_url_without_auth(" "), "");
    }
}
