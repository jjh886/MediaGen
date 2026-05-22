use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tracing::debug;

use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::types::StepResult;

pub(crate) mod proxy;
pub(crate) mod sse;

#[cfg(test)]
#[path = "http_engine_tests.rs"]
mod tests;

pub struct HttpEngine {
    client: reqwest::Client,
}

struct RequestConfig {
    method: Method,
    method_label: String,
    url: String,
    timeout_ms: u64,
    headers: HeaderMap,
}

impl HttpEngine {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("failed to build reqwest client");
        Self { client }
    }

    async fn do_request(&self, params: &Value) -> Result<StepResult, AppError> {
        let config = parse_request_config(params)?;
        let client = client_for_request(&self.client, &config.url, params)?;
        let req = build_request(&client, &config, params)?;
        debug!(url = %config.url, method = %config.method_label, "发送 HTTP 请求");
        let resp = req.send().await?;
        let result = response_to_step_result(resp).await?;
        debug!(
            status = result.data["status"].as_u64().unwrap_or_default(),
            "收到 HTTP 响应"
        );
        Ok(result)
    }

    async fn do_poll(&self, params: &Value) -> Result<StepResult, AppError> {
        let interval_ms = params["interval_ms"].as_u64().unwrap_or(3_000);
        let timeout_ms = params["timeout_ms"].as_u64().unwrap_or(180_000);
        let until = params["until"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("http.poll 缺少 until 条件".into()))?
            .to_string();

        let req_params = params.clone();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            let result = self.do_request(&req_params).await?;

            if let Some(body) = result.data.get("body") {
                if evaluate_condition(body, &until) {
                    debug!("轮询条件满足");
                    return Ok(result);
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Ok(StepResult {
                    success: false,
                    data: result.data,
                    error: Some(format!("轮询超时 ({timeout_ms}ms)")),
                });
            }

            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
    }

    async fn do_download(&self, params: &Value) -> Result<StepResult, AppError> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("http.download 缺少 url 参数".into()))?;

        let output_dir = params["output_dir"]
            .as_str()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);

        let mut headers = build_headers(&params["headers"]);
        if let Some(cookies) = params["cookies"].as_str() {
            if !cookies.is_empty() {
                headers.insert(
                    reqwest::header::COOKIE,
                    HeaderValue::from_str(cookies)
                        .map_err(|e| AppError::PlatformError(format!("cookie 格式错误: {e}")))?,
                );
            }
        }

        debug!(url = url, "开始下载文件");

        let resp = self.client.get(url).headers(headers).send().await?;

        if !resp.status().is_success() {
            return Ok(StepResult {
                success: false,
                data: json!({}),
                error: Some(format!("下载失败: HTTP {}", resp.status().as_u16())),
            });
        }

        // 从 URL 或 Content-Disposition 推断文件名
        let filename = extract_filename(url, resp.headers());
        let file_path = output_dir.join(&filename);

        let bytes = resp.bytes().await?;
        let size = bytes.len() as u64;

        let mut file = tokio::fs::File::create(&file_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;

        debug!(path = %file_path.display(), size = size, "文件下载完成");

        Ok(StepResult {
            success: true,
            data: json!({
                "path": file_path.to_string_lossy(),
                "size": size,
            }),
            error: None,
        })
    }
}

#[async_trait]
impl AtomExecutor for HttpEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "http"
    }

    async fn execute(&self, action: &str, params: &Value) -> Result<StepResult, AppError> {
        match action {
            "http.request" => self.do_request(params).await,
            "http.sse_request" => sse::do_sse_request(&self.client, params).await,
            "http.poll" => self.do_poll(params).await,
            "http.download" => self.do_download(params).await,
            _ => Ok(StepResult {
                success: false,
                data: json!({}),
                error: Some(format!("未知的 HTTP 操作: {action}")),
            }),
        }
    }
}

// ── 辅助函数 ──

fn parse_request_config(params: &Value) -> Result<RequestConfig, AppError> {
    let method_label = params["method"].as_str().unwrap_or("GET").to_uppercase();
    let url = params["url"]
        .as_str()
        .ok_or_else(|| AppError::PlatformError("http.request 缺少 url 参数".into()))?
        .to_string();
    let method = method_label
        .parse()
        .map_err(|_| AppError::PlatformError(format!("不支持的 HTTP 方法: {method_label}")))?;
    Ok(RequestConfig {
        method,
        method_label,
        url,
        timeout_ms: params["timeout_ms"].as_u64().unwrap_or(30_000),
        headers: build_request_headers(params)?,
    })
}

fn build_request_headers(params: &Value) -> Result<HeaderMap, AppError> {
    let mut headers = build_headers(&params["headers"]);
    if let Some(cookies) = params["cookies"].as_str().filter(|v| !v.is_empty()) {
        headers.insert(
            reqwest::header::COOKIE,
            HeaderValue::from_str(cookies)
                .map_err(|e| AppError::PlatformError(format!("cookie 格式错误: {e}")))?,
        );
    }
    Ok(headers)
}

fn client_for_request(
    default_client: &reqwest::Client,
    url: &str,
    params: &Value,
) -> Result<reqwest::Client, AppError> {
    let Some(proxy_url) = proxy::proxy_url_for_request(url, params) else {
        return Ok(default_client.clone());
    };
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .proxy(
            reqwest::Proxy::all(&proxy_url)
                .map_err(|e| AppError::PlatformError(format!("代理地址无效 {proxy_url}: {e}")))?,
        )
        .build()?)
}

fn build_request(
    client: &reqwest::Client,
    config: &RequestConfig,
    params: &Value,
) -> Result<reqwest::RequestBuilder, AppError> {
    let req = client
        .request(config.method.clone(), &config.url)
        .headers(config.headers.clone())
        .timeout(Duration::from_millis(config.timeout_ms));
    apply_optional_body(req, params)
}

fn apply_optional_body(
    req: reqwest::RequestBuilder,
    params: &Value,
) -> Result<reqwest::RequestBuilder, AppError> {
    match request_body_value(params)? {
        Some(body) => apply_request_body(req, params, body),
        None => Ok(req),
    }
}

fn request_body_value<'a>(params: &'a Value) -> Result<Option<&'a Value>, AppError> {
    if let Some(body) = params.get("body").filter(|body| !body.is_null()) {
        return Ok(Some(body));
    }
    let Some(body_path) = params.get("body_path").and_then(|value| value.as_str()) else {
        return Ok(None);
    };
    let context = params.get("context").unwrap_or(params);
    lookup_dot_value(context, body_path)
        .map(Some)
        .ok_or_else(|| {
            AppError::PlatformError(format!("http.request 找不到 body_path: {body_path}"))
        })
}

async fn response_to_step_result(resp: reqwest::Response) -> Result<StepResult, AppError> {
    let status = resp.status().as_u16();
    let success = resp.status().is_success();
    let resp_headers = response_headers(resp.headers());
    let body_text = resp.text().await?;
    let body_value: Value =
        serde_json::from_str(&body_text).unwrap_or_else(|_| Value::String(body_text.clone()));
    Ok(StepResult {
        success,
        data: json!({
            "status": status,
            "body": body_value,
            "body_text": body_text,
            "headers": resp_headers,
        }),
        error: http_result_error(success, status, &body_text),
    })
}

fn response_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect()
}

fn http_result_error(success: bool, status: u16, body_text: &str) -> Option<String> {
    if success {
        None
    } else {
        Some(http_error_message(status, body_text))
    }
}

fn build_headers(headers_value: &Value) -> HeaderMap {
    let mut map = HeaderMap::new();
    if let Some(obj) = headers_value.as_object() {
        for (k, v) in obj {
            if let (Ok(name), Some(val_str)) = (k.parse::<HeaderName>(), v.as_str()) {
                if let Ok(val) = HeaderValue::from_str(val_str) {
                    map.insert(name, val);
                }
            }
        }
    }
    map
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestBodyFormat {
    Json,
    Form,
    Text,
}

fn body_format(params: &Value) -> RequestBodyFormat {
    match params["body_format"].as_str().unwrap_or("json") {
        "form" | "x-www-form-urlencoded" => RequestBodyFormat::Form,
        "text" | "raw" => RequestBodyFormat::Text,
        _ => RequestBodyFormat::Json,
    }
}

fn apply_request_body(
    req: reqwest::RequestBuilder,
    params: &Value,
    body: &Value,
) -> Result<reqwest::RequestBuilder, AppError> {
    match body_format(params) {
        RequestBodyFormat::Json => Ok(req.json(body)),
        RequestBodyFormat::Text => Ok(req.body(value_to_string(body))),
        RequestBodyFormat::Form => Ok(req.form(&form_pairs(body)?)),
    }
}

fn form_pairs(body: &Value) -> Result<Vec<(String, String)>, AppError> {
    let obj = body
        .as_object()
        .ok_or_else(|| AppError::PlatformError("http.request form body 必须是对象".into()))?;
    Ok(obj
        .iter()
        .filter_map(|(key, value)| value_to_optional_string(value).map(|v| (key.clone(), v)))
        .collect())
}

fn value_to_optional_string(value: &Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    Some(value_to_string(value))
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        other => other.to_string(),
    }
}

fn http_error_message(status: u16, body_text: &str) -> String {
    let body = body_text.trim();
    if body.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {}", truncate_message(body, 800))
    }
}

fn truncate_message(message: &str, max_chars: usize) -> String {
    let mut iter = message.chars();
    let truncated: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn extract_filename(url: &str, headers: &HeaderMap) -> String {
    // 尝试从 Content-Disposition 获取
    if let Some(cd) = headers.get("content-disposition") {
        if let Ok(cd_str) = cd.to_str() {
            if let Some(pos) = cd_str.find("filename=") {
                let name = cd_str[pos + 9..].trim_matches('"').trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    // 从 URL 路径取最后一段
    url.rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .filter(|s| !s.is_empty() && s.contains('.'))
        .unwrap_or("download.bin")
        .to_string()
}

/// 简单条件求值：支持 `path == 'value'` 和 `path != 'value'`
fn evaluate_condition(data: &Value, condition: &str) -> bool {
    let (path, expected, negate) = if let Some((left, right)) = condition.split_once("!=") {
        (
            left.trim(),
            right.trim().trim_matches('\'').trim_matches('"'),
            true,
        )
    } else if let Some((left, right)) = condition.split_once("==") {
        (
            left.trim(),
            right.trim().trim_matches('\'').trim_matches('"'),
            false,
        )
    } else {
        return false;
    };

    let actual = resolve_dot_path(data, path);
    let actual_str = match &actual {
        Value::String(s) => s.as_str(),
        Value::Null => "",
        other => {
            // 数字/布尔等先转文本比较
            let s = other.to_string();
            let matches = s.trim_matches('"') == expected;
            return if negate { !matches } else { matches };
        }
    };

    let matches = actual_str == expected;
    if negate {
        !matches
    } else {
        matches
    }
}

fn resolve_dot_path<'a>(data: &'a Value, path: &str) -> &'a Value {
    let mut current = data;
    for key in path.split('.') {
        match current.get(key) {
            Some(v) => current = v,
            None => return &Value::Null,
        }
    }
    current
}

fn lookup_dot_value<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = data;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current)
}
