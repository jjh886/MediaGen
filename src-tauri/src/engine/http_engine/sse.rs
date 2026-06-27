use serde_json::{json, Value};
use tracing::debug;

use super::{
    build_request, client_for_request, http_result_error, parse_request_config, response_headers,
};
use crate::engine::retry::{
    is_retryable_http_status, is_transient_network_error, sleep_async, RetryConfig,
};
use crate::error::AppError;
use crate::types::StepResult;

pub(super) async fn do_sse_request(
    default_client: &reqwest::Client,
    params: &Value,
) -> Result<StepResult, AppError> {
    let config = parse_request_config(params)?;
    let client = client_for_request(default_client, &config.url, params)?;
    let retry = RetryConfig::from_params(params, 2, &[2_000, 5_000]);
    let mut last_error: Option<AppError> = None;
    for attempt in 1..=retry.attempts() {
        let req = build_request(&client, &config, params)?;
        debug!(url = %config.url, method = %config.method_label, attempt, "发送 SSE HTTP 请求");
        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(e) if attempt < retry.attempts() && is_transient_network_error(&e.to_string()) => {
                last_error = Some(AppError::Http(e));
                sleep_async(&retry, attempt).await;
                continue;
            }
            Err(e) => return Err(AppError::Http(e)),
        };
        let status = resp.status().as_u16();
        let success = resp.status().is_success();
        let headers = response_headers(resp.headers());
        let body_text = resp.text().await?;
        if attempt < retry.attempts() && is_retryable_http_status(status) {
            sleep_async(&retry, attempt).await;
            continue;
        }
        let events = collect_sse_events(&body_text);
        let body = serde_json::from_str::<Value>(&body_text)
            .unwrap_or_else(|_| Value::String(body_text.clone()));
        return Ok(StepResult {
            success,
            data: json!({
                "status": status,
                "body": body,
                "body_text": body_text,
                "headers": headers,
                "events": events,
            }),
            error: http_result_error(success, status, &body_text),
        });
    }
    Err(last_error
        .unwrap_or_else(|| AppError::PlatformError(format!("SSE HTTP 请求失败: {}", config.url))))
}

pub(crate) fn collect_sse_events(raw_text: &str) -> Vec<Value> {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return vec![value];
    }

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
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    serde_json::from_str(&data).ok()
}
