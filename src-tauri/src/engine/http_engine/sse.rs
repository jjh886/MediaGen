use serde_json::{json, Value};
use tracing::debug;

use super::{
    build_request, client_for_request, http_result_error, parse_request_config, response_headers,
};
use crate::error::AppError;
use crate::types::StepResult;

pub(super) async fn do_sse_request(
    default_client: &reqwest::Client,
    params: &Value,
) -> Result<StepResult, AppError> {
    let config = parse_request_config(params)?;
    let client = client_for_request(default_client, &config.url, params)?;
    let req = build_request(&client, &config, params)?;
    debug!(url = %config.url, method = %config.method_label, "发送 SSE HTTP 请求");
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let success = resp.status().is_success();
    let headers = response_headers(resp.headers());
    let body_text = resp.text().await?;
    let events = collect_sse_events(&body_text);
    let body = serde_json::from_str::<Value>(&body_text)
        .unwrap_or_else(|_| Value::String(body_text.clone()));
    Ok(StepResult {
        success,
        data: json!({
            "status": status,
            "body": body,
            "body_text": body_text,
            "headers": headers,
            "events": events,
        }),
        error: http_result_error(success, status, &body_text),
    })
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
