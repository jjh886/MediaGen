use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use base64::prelude::{Engine as _, BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD};
use futures_util::StreamExt;
use mediagen_browser_gateway::BrowserFingerprint;
use mediagen_plugin_runtime::{PluginHost, PluginStepResult};
use rand::RngCore;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::callback_host::CallbackRegistry;
use crate::net_client::normalize_proxy_url;
use crate::port_process;
use crate::store::AccountRecord;

#[path = "plugin_http_stream.rs"]
mod http_stream;
use http_stream::read_response_bounded;
#[path = "plugin_http_body.rs"]
mod http_body;
use http_body::apply_body;
#[path = "plugin_file_io.rs"]
mod file_io;
#[path = "plugin_http_request.rs"]
mod http_request;
#[path = "plugin_http_transport.rs"]
mod http_transport;

const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Clone)]
pub struct ServerPluginHost {
    credential_cipher: Option<std::sync::Arc<crate::generation_queue::CredentialCipher>>,
    protected_files: Vec<PathBuf>,
    profile_dir: PathBuf,
    checkpoint_memory: std::sync::Arc<std::sync::Mutex<Value>>,
    pub(super) submitted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) emitted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    events: Option<tokio::sync::mpsc::Sender<Value>>,
    terminal_event: std::sync::Arc<std::sync::Mutex<Option<Value>>>,
    request_id: String,
    http_profile: String,
    profile_clients: std::sync::Arc<std::sync::Mutex<HashMap<String, wreq::Client>>>,
    locks: crate::plugin_locks::PluginLocks,
    callbacks: CallbackRegistry,
    client: reqwest::Client,
    proxy_url: String,
    browser_fingerprint: Option<BrowserFingerprint>,
}

impl ServerPluginHost {
    pub fn with_events(
        mut self,
        events: Option<tokio::sync::mpsc::Sender<Value>>,
        request_id: String,
    ) -> Self {
        self.events = events;
        self.request_id = request_id;
        self
    }

    pub(crate) fn take_terminal_event(&self) -> Option<Value> {
        self.terminal_event
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }

    pub(crate) fn with_http_profile(mut self, manifest: &mediagen_core::PluginManifest) -> Self {
        self.http_profile = manifest
            .extra
            .get("http_profile")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self
    }

    pub fn new_for_account(account: &AccountRecord) -> Self {
        Self::with_fingerprint(
            account.proxy_url.clone(),
            BrowserFingerprint::from_values(&account.browser_os, &account.browser_version),
        )
    }

    pub fn new_with_metadata(
        proxy_url: String,
        metadata: &std::collections::HashMap<String, Value>,
    ) -> Self {
        Self::with_fingerprint(proxy_url, BrowserFingerprint::from_metadata(metadata))
    }

    fn with_fingerprint(
        proxy_url: String,
        browser_fingerprint: Option<BrowserFingerprint>,
    ) -> Self {
        Self {
            credential_cipher: None,
            protected_files: Vec::new(),
            profile_dir: PathBuf::new(),
            checkpoint_memory: Default::default(),
            emitted: Default::default(),
            submitted: Default::default(),
            events: None,
            terminal_event: Default::default(),
            request_id: String::new(),
            http_profile: String::new(),
            profile_clients: Default::default(),
            locks: Default::default(),
            callbacks: CallbackRegistry::default(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            proxy_url,
            browser_fingerprint,
        }
    }

    async fn execute(&self, action: &str, params: &Value) -> Result<Value, String> {
        match action {
            "checkpoint" => self.checkpoint(params).await,
            "stream.emit" => {
                if self.events.is_some() {
                    self.emitted
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if matches!(
                    params["event"]["type"].as_str(),
                    Some("response.completed" | "response.incomplete" | "response.failed")
                ) {
                    *self
                        .terminal_event
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(params["event"].clone());
                    return Ok(Value::Null);
                }
                if let Some(sender) = &self.events {
                    sender
                        .send(params["event"].clone())
                        .await
                        .map_err(|_| "请求已取消".to_string())?;
                }
                Ok(Value::Null)
            }
            "lock.acquire" => {
                self.locks.acquire(&required_str(params, "key")?).await?;
                Ok(Value::Null)
            }
            "lock.release" => {
                self.locks.release(&required_str(params, "key")?);
                Ok(Value::Null)
            }
            "file.read_json" => self.read_json(params).await,
            "file.write_json" => self.write_json(params).await,
            "file.read_base64" => self.read_base64(params).await,
            "file.inspect_image" => self.inspect_image(params).await,
            "http.request" => self.http_request(params, false).await,
            "http.sse_request" => self.http_request(params, true).await,
            "crypto.random_base64url" => random_base64url(params),
            "crypto.sha256_base64url" => sha256_base64url(params),
            "crypto.base64_encode_utf8" => base64_encode_utf8(params),
            "crypto.base64_encode_latin1" => base64_encode_latin1(params),
            "crypto.base64url_decode_utf8" => base64url_decode_utf8(params),
            "crypto.base64_decode_latin1" => base64_decode_latin1(params),
            "time.sleep" => sleep_for(params).await,
            "process.kill_port" => port_process::kill_port(params),
            "audio.transcode" => crate::audio_transcode::transcode(params).await,
            "process.run" => crate::plugin_process::run(params).await,
            "process.spawn" => port_process::spawn_process(params),
            "external_cdp.list_targets" => crate::external_cdp::list_targets(params).await,
            "external_cdp.create_target" => crate::external_cdp::create_target(params).await,
            "external_cdp.close_target" => crate::external_cdp::close_target(params).await,
            "external_cdp.register_target" => crate::external_cdp::register_target(params),
            "external_cdp.unregister_target" => crate::external_cdp::unregister_target(params),
            "external_cdp.evaluate" => crate::external_cdp::evaluate(params).await,
            "external_cdp.command" => crate::external_cdp::command(params).await,
            action if action.starts_with("callback_server.") => {
                self.callbacks.execute(action, params).await
            }
            other => Err(format!("服务端插件 host 暂不支持 action: {other}")),
        }
    }

    async fn http_request(&self, params: &Value, sse: bool) -> Result<Value, String> {
        let response = self.send_http(params).await?;
        let status = response.status;
        let success = (200..300).contains(&status);
        let headers = response.headers.clone();
        if params["response_body_format"].as_str() == Some("base64") {
            let bytes = read_response_bounded(response).await?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(format!("HTTP 二进制响应超过限制 {} bytes", MAX_FILE_BYTES));
            }
            return Ok(json!({
                "status": status,
                "body_base64": BASE64_STANDARD.encode(bytes),
                "headers": headers,
                "ok": success,
            }));
        }
        let body_text =
            String::from_utf8_lossy(&read_response_bounded(response).await?).into_owned();
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
}

#[async_trait]
impl PluginHost for ServerPluginHost {
    async fn stream(
        &self,
        action: &str,
        params: &Value,
        events: tokio::sync::mpsc::Sender<Value>,
    ) -> PluginStepResult {
        let result = if action == "http.sse_request" {
            self.stream_request(params, events).await
        } else {
            Err("host.stream 只支持 http.sse_request".to_string())
        };
        match result {
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

fn insert_header_if_missing(
    headers: &mut HeaderMap,
    name: HeaderName,
    value: String,
) -> Result<(), String> {
    if !headers.contains_key(&name) {
        headers.insert(
            name,
            HeaderValue::from_str(&value).map_err(|err| err.to_string())?,
        );
    }
    Ok(())
}

fn response_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut result = HashMap::<String, String>::new();
    for (name, value) in headers.iter() {
        let text = value.to_str().unwrap_or("");
        result
            .entry(name.to_string())
            .and_modify(|existing| {
                existing.push_str(if name == reqwest::header::SET_COOKIE {
                    "\n"
                } else {
                    ", "
                });
                existing.push_str(text);
            })
            .or_insert_with(|| text.to_string());
    }
    result
}

async fn read_limited_file(path: &str, max_bytes: Option<u64>) -> Result<Vec<u8>, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|err| format!("找不到文件 {path}: {err}"))?;
    let limit = max_bytes.unwrap_or(MAX_FILE_BYTES);
    if metadata.len() > limit {
        return Err(format!("文件超过限制 {limit} bytes: {path}"));
    }
    tokio::fs::read(path).await.map_err(|err| err.to_string())
}

fn proxy_url(params: &Value, account_proxy: &str) -> Option<String> {
    if params["proxy"].as_str() == Some("direct")
        || params["use_proxy"].as_bool() == Some(false)
        || params["use_account_proxy"].as_bool() == Some(false)
    {
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

fn base64_encode_utf8(params: &Value) -> Result<Value, String> {
    let text = required_str(params, "text")?;
    Ok(json!({ "value": BASE64_STANDARD.encode(text.as_bytes()) }))
}

fn base64_encode_latin1(params: &Value) -> Result<Value, String> {
    let text = required_str(params, "text")?;
    let bytes = text
        .chars()
        .map(|value| {
            u8::try_from(value as u32)
                .map_err(|_| "base64 latin1 编码遇到超出 0-255 的字符".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({ "value": BASE64_STANDARD.encode(bytes) }))
}

fn base64_decode_latin1(params: &Value) -> Result<Value, String> {
    let text = required_str(params, "text")?;
    let bytes = BASE64_URL_SAFE_NO_PAD
        .decode(&text)
        .or_else(|_| BASE64_STANDARD.decode(&text))
        .map_err(|err| format!("base64 latin1 解码失败: {err}"))?;
    let value = bytes.into_iter().map(char::from).collect::<String>();
    Ok(json!({ "value": value }))
}

async fn sleep_for(params: &Value) -> Result<Value, String> {
    let milliseconds = params["milliseconds"].as_u64().unwrap_or(0).min(60_000);
    tokio::time::sleep(Duration::from_millis(milliseconds)).await;
    Ok(json!({ "milliseconds": milliseconds }))
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
#[path = "server_plugin_host_tests.rs"]
mod tests;
