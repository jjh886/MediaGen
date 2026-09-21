use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::cdp::CdpClient;
use crate::{BrowserGatewayError, BrowserSession};

pub(crate) fn runtime_signature(session: &BrowserSession) -> String {
    use sha2::{Digest, Sha256};
    let values = [
        "proxy_server",
        "proxy_username",
        "proxy_password",
        "browser_os",
        "browser_version",
    ]
    .map(|key| session.metadata.get(key).cloned().unwrap_or(Value::Null));
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&values).unwrap_or_default())
    )
}

pub(crate) async fn runtime_config_matches(
    path: &std::path::Path,
    session: &BrowserSession,
) -> bool {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    value["settings"].as_str() == Some(runtime_signature(session).as_str())
}

#[derive(Debug, Clone)]
struct ProxyAuth {
    username: String,
    password: String,
}

pub(crate) fn proxy_server_from_session(session: &BrowserSession) -> Option<String> {
    session
        .metadata
        .get("proxy_server")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(proxy_server_without_auth)
}

pub(crate) async fn enable_auth_handling(
    client: &Arc<CdpClient>,
) -> Result<(), BrowserGatewayError> {
    // 只启用认证事件处理；如果 Chromium 仍暂停普通请求，manager 会立即 continueRequest。
    client.send("Fetch.enable", auth_handling_params()).await?;
    Ok(())
}

pub(crate) async fn continue_auth(
    client: &Arc<CdpClient>,
    session: &BrowserSession,
    params: &Value,
) {
    let Some(request_id) = params.get("requestId").and_then(Value::as_str) else {
        return;
    };
    let source = params
        .get("authChallenge")
        .and_then(|challenge| challenge.get("source"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let response = auth_response_for_challenge(&session.metadata, source);
    let _ = client
        .send(
            "Fetch.continueWithAuth",
            json!({
                "requestId": request_id,
                "authChallengeResponse": response,
            }),
        )
        .await;
}

fn auth_response_for_challenge(metadata: &HashMap<String, Value>, source: &str) -> Value {
    if source == "Proxy" {
        if let Some(auth) = proxy_auth_from_metadata(metadata) {
            return json!({
                "response": "ProvideCredentials",
                "username": auth.username,
                "password": auth.password,
            });
        }
    }
    json!({ "response": "CancelAuth" })
}

fn auth_handling_params() -> Value {
    json!({
        "patterns": [{ "urlPattern": "*" }],
        "handleAuthRequests": true
    })
}

fn proxy_auth_from_metadata(metadata: &HashMap<String, Value>) -> Option<ProxyAuth> {
    if let (Some(username), Some(password)) = (
        text_field(metadata, "proxy_username"),
        text_field(metadata, "proxy_password"),
    ) {
        return Some(ProxyAuth { username, password });
    }
    let raw = metadata.get("proxy_server")?.as_str()?;
    auth_from_proxy_url(raw)
}

fn text_field(metadata: &HashMap<String, Value>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn auth_from_proxy_url(raw: &str) -> Option<ProxyAuth> {
    let (_, rest) = raw.split_once("://")?;
    let (auth, _) = rest.split_once('@')?;
    let (username, password) = auth.split_once(':').unwrap_or((auth, ""));
    if username.trim().is_empty() {
        return None;
    }
    Some(ProxyAuth {
        username: decode_url_part(username),
        password: decode_url_part(password),
    })
}

fn proxy_server_without_auth(raw: &str) -> String {
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("socks5://{raw}")
    };
    let Some((scheme, rest)) = with_scheme.split_once("://") else {
        return with_scheme;
    };
    let host_part = rest.rsplit_once('@').map(|(_, host)| host).unwrap_or(rest);
    format!("{scheme}://{host_part}")
}

fn decode_url_part(value: &str) -> String {
    reqwest::Url::parse(&format!("http://local/?v={}", value.replace('+', "%2B")))
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .next()
                .map(|(_, value)| value.into_owned())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_auth_from_proxy_server() {
        assert_eq!(
            proxy_server_without_auth("socks5://user:pass@proxy.test:1080"),
            "socks5://proxy.test:1080"
        );
        assert_eq!(
            proxy_server_without_auth("proxy.test:1080"),
            "socks5://proxy.test:1080"
        );
    }

    #[test]
    fn parses_proxy_auth_from_encoded_url() {
        let auth = auth_from_proxy_url("socks5://user%40a:p%3Aa%2Fss@proxy.test:1080").unwrap();

        assert_eq!(auth.username, "user@a");
        assert_eq!(auth.password, "p:a/ss");
        assert!(auth_from_proxy_url("socks5://:pass@proxy.test:1080").is_none());
        assert!(auth_from_proxy_url("proxy.test:1080").is_none());
    }

    #[test]
    fn auth_response_provides_proxy_credentials_and_cancels_server_auth() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "proxy_server".to_string(),
            json!("socks5://user:pass@proxy.test:1080"),
        );

        let proxy_response = auth_response_for_challenge(&metadata, "Proxy");
        assert_eq!(proxy_response["response"], "ProvideCredentials");
        assert_eq!(proxy_response["username"], "user");

        let server_response = auth_response_for_challenge(&metadata, "Server");
        assert_eq!(server_response["response"], "CancelAuth");
        let missing_proxy_response = auth_response_for_challenge(&HashMap::new(), "Proxy");
        assert_eq!(missing_proxy_response["response"], "CancelAuth");
    }

    #[test]
    fn auth_handling_uses_non_empty_fetch_pattern() {
        let params = auth_handling_params();
        assert_eq!(params["handleAuthRequests"], true);
        assert_eq!(params["patterns"][0]["urlPattern"], "*");
    }
}
