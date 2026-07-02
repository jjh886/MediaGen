use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use base64::prelude::{Engine as _, BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD};
use mediagen_plugin_runtime::{PluginHost, PluginStepResult};
use rand::RngCore;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::callback_host::CallbackRegistry;
use crate::net_client::normalize_proxy_url;
use crate::port_process;

const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Clone)]
pub struct ServerPluginHost {
    callbacks: CallbackRegistry,
    client: reqwest::Client,
    proxy_url: String,
}

impl ServerPluginHost {
    pub fn new(proxy_url: String) -> Self {
        Self {
            callbacks: CallbackRegistry::default(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            proxy_url,
        }
    }

    async fn execute(&self, action: &str, params: &Value) -> Result<Value, String> {
        match action {
            "file.read_json" => self.read_json(params).await,
            "file.write_json" => self.write_json(params).await,
            "file.read_base64" => self.read_base64(params).await,
            "http.request" => self.http_request(params, false).await,
            "http.sse_request" => self.http_request(params, true).await,
            "crypto.random_base64url" => random_base64url(params),
            "crypto.sha256_base64url" => sha256_base64url(params),
            "crypto.base64url_decode_utf8" => base64url_decode_utf8(params),
            "process.kill_port" => port_process::kill_port(params),
            action if action.starts_with("callback_server.") => {
                self.callbacks.execute(action, params).await
            }
            other => Err(format!("服务端插件 host 暂不支持 action: {other}")),
        }
    }

    async fn read_json(&self, params: &Value) -> Result<Value, String> {
        let path = required_str(params, "path")?;
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|err| format!("file.read_json 读取失败 {path}: {err}"))?;
        serde_json::from_str(&content).map_err(|err| format!("file.read_json 解析失败: {err}"))
    }

    async fn write_json(&self, params: &Value) -> Result<Value, String> {
        let path = PathBuf::from(required_str(params, "path")?);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| err.to_string())?;
        }
        let value = params
            .get("value")
            .cloned()
            .ok_or_else(|| "file.write_json 缺少 value".to_string())?;
        let content = serde_json::to_vec_pretty(&value).map_err(|err| err.to_string())?;
        tokio::fs::write(&path, [&content[..], b"\n"].concat())
            .await
            .map_err(|err| err.to_string())?;
        Ok(json!({ "path": path.to_string_lossy(), "bytes": content.len() + 1 }))
    }

    async fn read_base64(&self, params: &Value) -> Result<Value, String> {
        let path = required_str(params, "path")?;
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|err| format!("file.read_base64 找不到文件 {path}: {err}"))?;
        let max_bytes = params["max_bytes"].as_u64().unwrap_or(MAX_FILE_BYTES);
        if metadata.len() > max_bytes {
            return Err(format!(
                "file.read_base64 文件超过限制 {max_bytes} bytes: {path}"
            ));
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|err| err.to_string())?;
        Ok(json!({
            "path": path,
            "base64": BASE64_STANDARD.encode(&bytes),
            "mime": mime_type(&path),
            "size": bytes.len(),
        }))
    }

    async fn http_request(&self, params: &Value, sse: bool) -> Result<Value, String> {
        let request = self.build_request(params).await?;
        let response = request.send().await.map_err(|err| err.to_string())?;
        let status = response.status().as_u16();
        let success = response.status().is_success();
        let headers = response_headers(response.headers());
        let body_text = response.text().await.map_err(|err| err.to_string())?;
        let body = serde_json::from_str::<Value>(&body_text)
            .unwrap_or_else(|_| Value::String(body_text.clone()));
        Ok(json!({
            "status": status,
            "body": body,
            "body_text": body_text,
            "headers": headers,
            "events": if sse { collect_sse_events(&body_text) } else { Vec::<Value>::new() },
            "ok": success,
        }))
    }

    async fn build_request(&self, params: &Value) -> Result<reqwest::RequestBuilder, String> {
        let method = params["method"].as_str().unwrap_or("GET");
        let url = required_str(params, "url")?;
        let client = self.client_for(params)?;
        let mut request = client
            .request(
                method
                    .parse()
                    .map_err(|_| format!("不支持的 HTTP 方法: {method}"))?,
                url,
            )
            .headers(request_headers(params)?)
            .timeout(Duration::from_millis(
                params["timeout_ms"].as_u64().unwrap_or(30_000),
            ));
        if let Some(body) = params.get("body").filter(|value| !value.is_null()) {
            request = apply_body(request, params, body)?;
        }
        Ok(request)
    }

    fn client_for(&self, params: &Value) -> Result<reqwest::Client, String> {
        let Some(proxy_url) = proxy_url(params, &self.proxy_url) else {
            return Ok(self.client.clone());
        };
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .proxy(
                reqwest::Proxy::all(&proxy_url)
                    .map_err(|err| format!("代理地址无效 {proxy_url}: {err}"))?,
            )
            .build()
            .map_err(|err| err.to_string())
    }
}

#[async_trait]
impl PluginHost for ServerPluginHost {
    async fn call(&self, action: &str, params: &Value) -> PluginStepResult {
        match self.execute(action, params).await {
            Ok(data) => PluginStepResult {
                success: true,
                data,
                error: None,
            },
            Err(error) => PluginStepResult {
                success: false,
                data: Value::Null,
                error: Some(error),
            },
        }
    }
}

fn collect_sse_events(raw_text: &str) -> Vec<Value> {
    raw_text
        .replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(parse_sse_block)
        .collect()
}

fn parse_sse_block(block: &str) -> Option<Value> {
    let data = block
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
        .filter(|line| !line.is_empty() && *line != "[DONE]")
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&data).ok()
}

fn request_headers(params: &Value) -> Result<HeaderMap, String> {
    let mut map = HeaderMap::new();
    if let Some(headers) = params.get("headers").and_then(Value::as_object) {
        for (key, value) in headers {
            if let Some(value) = value.as_str() {
                map.insert(
                    key.parse::<HeaderName>().map_err(|err| err.to_string())?,
                    HeaderValue::from_str(value).map_err(|err| err.to_string())?,
                );
            }
        }
    }
    Ok(map)
}

fn response_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_str().unwrap_or("").to_string()))
        .collect()
}

fn apply_body(
    request: reqwest::RequestBuilder,
    params: &Value,
    body: &Value,
) -> Result<reqwest::RequestBuilder, String> {
    match params["body_format"].as_str().unwrap_or("json") {
        "form" | "x-www-form-urlencoded" => {
            let object = body
                .as_object()
                .ok_or_else(|| "form body 必须是对象".to_string())?;
            let pairs = object
                .iter()
                .map(|(key, value)| (key.clone(), value_to_string(value)))
                .collect::<Vec<_>>();
            Ok(request.form(&pairs))
        }
        "text" | "raw" => Ok(request.body(value_to_string(body))),
        _ => Ok(request.json(body)),
    }
}

fn proxy_url(params: &Value, account_proxy: &str) -> Option<String> {
    if params["proxy"].as_str() == Some("direct") || params["use_proxy"].as_bool() == Some(false) {
        return None;
    }
    let direct = params["proxy_url"].as_str().unwrap_or("").trim();
    let system = params["proxy"].as_str() == Some("system") || params["use_system_proxy"] == true;
    let value = if !direct.is_empty() {
        direct
    } else if system || !account_proxy.trim().is_empty() {
        account_proxy.trim()
    } else {
        ""
    };
    normalize_proxy_url(value)
}

fn random_base64url(params: &Value) -> Result<Value, String> {
    let mut bytes = vec![0u8; params["bytes"].as_u64().unwrap_or(32).clamp(1, 256) as usize];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    Ok(json!({ "value": BASE64_URL_SAFE_NO_PAD.encode(bytes) }))
}

fn sha256_base64url(params: &Value) -> Result<Value, String> {
    let text = required_str(params, "text")?;
    Ok(json!({ "value": BASE64_URL_SAFE_NO_PAD.encode(Sha256::digest(text.as_bytes())) }))
}

fn base64url_decode_utf8(params: &Value) -> Result<Value, String> {
    let text = required_str(params, "text")?;
    let bytes = BASE64_URL_SAFE_NO_PAD
        .decode(&text)
        .or_else(|_| BASE64_STANDARD.decode(&text))
        .map_err(|err| format!("base64url 解码失败: {err}"))?;
    let value = String::from_utf8(bytes).map_err(|err| format!("base64url 不是 UTF-8: {err}"))?;
    Ok(json!({ "value": value }))
}

fn mime_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}

fn required_str(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("缺少 {key}"))
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::proxy_url;

    #[test]
    fn http_request_defaults_to_account_proxy() {
        assert_eq!(
            proxy_url(&json!({}), "127.0.0.1:1080"),
            Some("socks5://127.0.0.1:1080".to_string())
        );
    }

    #[test]
    fn http_request_can_opt_out_of_account_proxy() {
        assert_eq!(
            proxy_url(&json!({ "proxy": "direct" }), "socks5://127.0.0.1:1080"),
            None
        );
    }
}
