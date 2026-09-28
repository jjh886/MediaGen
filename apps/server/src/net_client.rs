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

/// 测试使用与生成请求相同的代理客户端；任何失败都不会降级为直连。
pub(crate) async fn test_proxy_connection(target: &str, proxy_url: &str) -> Result<u16, String> {
    use std::{error::Error, time::Duration};
    let client = http_client(proxy_url)?;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        client
            .get(target)
            .header(reqwest::header::USER_AGENT, "MediaGen proxy test")
            .send(),
    )
    .await
    .map_err(|_| "代理测试超时，请检查代理地址、端口和服务器网络".to_string())?;
    let response = match outcome {
        Ok(response) => response,
        Err(error) => {
            let mut causes = Vec::new();
            let mut source = error.source();
            while let Some(cause) = source.filter(|_| causes.len() < 8) {
                causes.push(cause.to_string());
                source = cause.source();
            }
            let detail = causes.join("；");
            if detail.to_ascii_lowercase().contains("tunnel") {
                // reqwest 不保留 CONNECT 失败状态码。仅向固定的公开测试地址探测，
                // 不将用户指定的 HTTPS URL 或业务请求降级为 HTTP。
                if let Ok(Ok(probe)) = tokio::time::timeout(
                    Duration::from_secs(5),
                    client.get("http://www.gstatic.com/generate_204").send(),
                )
                .await
                {
                    let status = probe.status().as_u16();
                    if status == 407 || status >= 600 {
                        return Err(proxy_rejected(status));
                    }
                }
            }
            return Err(format!("代理测试失败: {}；{}", error.without_url(), detail));
        }
    };
    let status = response.status().as_u16();
    if status == 407 || status >= 600 {
        return Err(proxy_rejected(status));
    }
    if !response.status().is_success() {
        return Err(format!(
            "已通过代理连接，但目标站点返回 HTTP {status}；请检查目标站点访问权限或代理出口状态"
        ));
    }
    Ok(status)
}
fn proxy_rejected(status: u16) -> String {
    format!("代理服务器拒绝请求（HTTP {status}），请核对协议、认证和供应商状态；未改用直连")
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
    async fn read_headers(socket: &mut tokio::net::TcpStream) -> String {
        use tokio::io::AsyncReadExt;
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 8192 {
            bytes.push(socket.read_u8().await.unwrap());
        }
        String::from_utf8(bytes).unwrap()
    }

    #[tokio::test]
    async fn proxy_test_sends_auth_and_reports_https_connect_rejection_code() {
        use tokio::io::AsyncWriteExt;
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = proxy.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for method in ["CONNECT", "GET"] {
                let (mut socket, _) = proxy.accept().await.unwrap();
                let headers = read_headers(&mut socket).await;
                assert!(headers.starts_with(method));
                assert!(headers
                    .to_ascii_lowercase()
                    .contains("proxy-authorization: basic dxnlcjpwyxnz"));
                socket.write_all(b"HTTP/1.1 615 Proxy Rejected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            }
        });
        let result = test_proxy_connection(
            "https://target.invalid/test",
            &format!("http://user:pass@{address}"),
        )
        .await;
        assert!(result.unwrap_err().contains("HTTP 615"));
        tokio::time::timeout(std::time::Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn plugin_native_and_fingerprint_transports_use_configured_authenticated_proxy() {
        use mediagen_plugin_runtime::PluginHost;
        use tokio::io::AsyncWriteExt;
        for profile in ["native", "chrome_149"] {
            let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = proxy.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = proxy.accept().await.unwrap();
                let request = read_headers(&mut socket).await;
                assert!(request.starts_with("GET http://target.invalid/proof "));
                assert!(request
                    .to_ascii_lowercase()
                    .contains("proxy-authorization: basic dxnlcjpwyxnz"));
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 18\r\nConnection: close\r\n\r\n{\"via_proxy\":true}").await.unwrap();
            });
            let host = crate::server_plugin_host::ServerPluginHost::new_with_metadata(
                format!("http://user:pass@{address}"),
                &Default::default(),
            );
            let result = host.call("http.request", &serde_json::json!({"url":"http://target.invalid/proof","tls_profile":profile,"timeout_ms":3000})).await;
            assert!(result.success, "{:?}", result.error);
            assert_eq!(result.data["body"]["via_proxy"], true);
            tokio::time::timeout(std::time::Duration::from_secs(3), server)
                .await
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn unavailable_proxy_never_falls_back_to_reachable_direct_origin() {
        use mediagen_plugin_runtime::PluginHost;
        let origin = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = proxy.local_addr().unwrap();
        drop(proxy);
        for profile in ["native", "chrome_149"] {
            let host = crate::server_plugin_host::ServerPluginHost::new_with_metadata(
                format!("http://{closed}"),
                &Default::default(),
            );
            let result = host.call("http.request", &serde_json::json!({"url":format!("http://{}/",origin.local_addr().unwrap()),"tls_profile":profile,"timeout_ms":1000})).await;
            assert!(!result.success);
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), origin.accept())
                .await
                .is_err()
        );
    }
}
