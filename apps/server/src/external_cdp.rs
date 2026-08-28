use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

static TASK_TARGETS: OnceLock<Mutex<HashMap<String, Vec<ExternalTarget>>>> = OnceLock::new();

#[derive(Clone)]
struct ExternalTarget {
    debug_http_url: String,
    target_id: String,
}

pub(crate) async fn list_targets(params: &Value) -> Result<Value, String> {
    let targets = fetch_targets(debug_http_url(params)).await?;
    Ok(Value::Array(
        targets
            .into_iter()
            .map(|target| {
                json!({
                    "id": target.get("id"),
                    "type": target.get("type"),
                    "title": target.get("title"),
                    "url": target.get("url"),
                })
            })
            .collect(),
    ))
}

pub(crate) async fn create_target(params: &Value) -> Result<Value, String> {
    let start_url = params
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "external_cdp.create_target 缺少 url".to_string())?;
    let endpoint = format!(
        "{}/json/new?{}",
        debug_http_url(params).trim_end_matches('/'),
        start_url
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .put(&endpoint)
        .send()
        .await
        .map_err(|error| format!("创建外部 CDP target 失败: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "创建外部 CDP target 返回 HTTP {}",
            response.status()
        ));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| format!("创建外部 CDP target 响应解析失败: {error}"))
}

pub(crate) async fn close_target(params: &Value) -> Result<Value, String> {
    let target_id = params
        .get("target_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "external_cdp.close_target 缺少 target_id".to_string())?;
    let endpoint = format!(
        "{}/json/close/{target_id}",
        debug_http_url(params).trim_end_matches('/')
    );
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?
        .get(endpoint)
        .send()
        .await
        .map_err(|error| format!("关闭外部 CDP target 失败: {error}"))?;
    Ok(json!({ "closed": response.status().is_success(), "target_id": target_id }))
}

pub(crate) fn register_target(params: &Value) -> Result<Value, String> {
    let task_id = required_text(params, "task_id")?;
    let target_id = required_text(params, "target_id")?;
    let target = ExternalTarget {
        debug_http_url: debug_http_url(params).to_string(),
        target_id: target_id.clone(),
    };
    task_targets()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .entry(task_id.clone())
        .or_default()
        .push(target);
    Ok(json!({ "task_id": task_id, "target_id": target_id }))
}

pub(crate) fn unregister_target(params: &Value) -> Result<Value, String> {
    let task_id = required_text(params, "task_id")?;
    let target_id = required_text(params, "target_id")?;
    let mut targets = task_targets()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(items) = targets.get_mut(&task_id) {
        items.retain(|target| target.target_id != target_id);
        if items.is_empty() {
            targets.remove(&task_id);
        }
    }
    Ok(json!({ "task_id": task_id, "target_id": target_id }))
}

pub(crate) async fn close_task_targets(task_id: &str) {
    let targets = task_targets()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(task_id)
        .unwrap_or_default();
    for target in targets {
        let _ = close_target(&json!({
            "debug_http_url": target.debug_http_url,
            "target_id": target.target_id,
        }))
        .await;
    }
}

/// 连接用户已授权的本机 Chromium CDP，平台只需声明目标和表达式。
pub(crate) async fn evaluate(params: &Value) -> Result<Value, String> {
    let expression = params
        .get("expression")
        .and_then(Value::as_str)
        .ok_or_else(|| "external_cdp.evaluate 缺少 expression".to_string())?;
    let runtime = send_target_command(
        params,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "awaitPromise": params.get("await_promise").and_then(Value::as_bool).unwrap_or(false),
            "returnByValue": true,
        }),
    )
    .await?;
    if let Some(exception) = runtime.get("exceptionDetails") {
        let message = exception
            .pointer("/exception/description")
            .or_else(|| exception.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("脚本执行失败");
        return Err(message.to_string());
    }
    Ok(runtime
        .pointer("/result/value")
        .cloned()
        .unwrap_or(Value::Null))
}

pub(crate) async fn command(params: &Value) -> Result<Value, String> {
    let method = params
        .get("method")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "external_cdp.command 缺少 method".to_string())?;
    let command_params = params.get("params").cloned().unwrap_or_else(|| json!({}));
    send_target_command(params, method, command_params).await
}

async fn send_target_command(
    selection: &Value,
    method: &str,
    command_params: Value,
) -> Result<Value, String> {
    let websocket_url = target_websocket_url(selection).await?;
    let timeout_ms = selection
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(30_000)
        .clamp(1, 30 * 60 * 1000);
    let (mut socket, _) = connect_async(&websocket_url)
        .await
        .map_err(|error| format!("连接 CDP target 失败: {error}"))?;
    socket
        .send(Message::Text(
            json!({
                "id": 1,
                "method": method,
                "params": command_params,
            })
            .to_string()
            .into(),
        ))
        .await
        .map_err(|error| error.to_string())?;
    let response = timeout(Duration::from_millis(timeout_ms), async {
        while let Some(message) = socket.next().await {
            let message = message.map_err(|error| error.to_string())?;
            let Ok(text) = message.into_text() else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
            if value.get("id").and_then(Value::as_u64) == Some(1) {
                return Ok::<Value, String>(value);
            }
        }
        Err("CDP websocket 在返回结果前关闭".to_string())
    })
    .await
    .map_err(|_| format!("external_cdp.evaluate 超时（{timeout_ms}ms）"))??;
    if let Some(error) = response.get("error") {
        return Err(format!("CDP {method} 失败: {error}"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| "CDP 响应缺少 result".to_string())
}

pub(crate) async fn target_websocket_url(params: &Value) -> Result<String, String> {
    let targets = fetch_targets(debug_http_url(params)).await?;
    let target = targets
        .iter()
        .find(|target| target_matches(target, params))
        .ok_or_else(|| target_not_found(&targets, params))?;
    target
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "CDP target 缺少 webSocketDebuggerUrl".to_string())
}

async fn fetch_targets(debug_url: &str) -> Result<Vec<Value>, String> {
    let url = format!("{}/json/list", debug_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("无法连接外部 CDP {url}: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("外部 CDP 返回 HTTP {}", response.status()));
    }
    response
        .json::<Vec<Value>>()
        .await
        .map_err(|error| format!("外部 CDP target 列表解析失败: {error}"))
}

fn debug_http_url(params: &Value) -> &str {
    params
        .get("debug_http_url")
        .and_then(Value::as_str)
        .unwrap_or("http://[::1]:9222")
}

fn task_targets() -> &'static Mutex<HashMap<String, Vec<ExternalTarget>>> {
    TASK_TARGETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn required_text(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("external_cdp 缺少 {key}"))
}

fn target_matches(target: &Value, params: &Value) -> bool {
    if let Some(expected) = params.get("target_id").and_then(Value::as_str) {
        if target.get("id").and_then(Value::as_str) != Some(expected) {
            return false;
        }
    }
    let target_type = target.get("type").and_then(Value::as_str).unwrap_or("");
    if let Some(expected) = params.get("target_type").and_then(Value::as_str) {
        if target_type != expected {
            return false;
        }
    }
    contains_filter(target, params, "url", "target_url_contains")
        && contains_filter(target, params, "title", "target_title_contains")
}

fn contains_filter(target: &Value, params: &Value, field: &str, filter: &str) -> bool {
    let Some(needle) = params.get(filter).and_then(Value::as_str) else {
        return true;
    };
    target
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| value.contains(needle))
}

fn target_not_found(targets: &[Value], params: &Value) -> String {
    let available = targets
        .iter()
        .filter_map(|target| target.get("url").and_then(Value::as_str))
        .take(8)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "没有找到匹配的外部 CDP target（url_contains={:?}, title_contains={:?}）；当前: {available}",
        params.get("target_url_contains"),
        params.get("target_title_contains")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_filters_are_platform_neutral() {
        let target = json!({ "type": "page", "url": "app://chat/page", "title": "Chat" });
        let params = json!({
            "target_type": "page",
            "target_url_contains": "chat",
            "target_title_contains": "Cha"
        });

        assert!(target_matches(&target, &params));
        assert!(!target_matches(
            &target,
            &json!({ "target_url_contains": "other" })
        ));
    }

    #[tokio::test]
    #[ignore = "需要本机已用 --remote-debugging-port=9222 启动 Chromium"]
    async fn live_cdp_lists_and_evaluates_page() {
        let targets = list_targets(&json!({})).await.unwrap();
        let page = targets
            .as_array()
            .unwrap()
            .iter()
            .find(|target| target["type"] == "page")
            .unwrap();
        let result = evaluate(&json!({
            "target_id": page["id"],
            "expression": "({title: document.title, href: location.href})"
        }))
        .await
        .unwrap();

        assert!(result["href"].as_str().is_some());

        let created = create_target(&json!({
            "url": "doubaowork://doubaowork-chat/chat"
        }))
        .await
        .unwrap();
        assert!(created["id"].as_str().is_some());
        let closed = close_target(&json!({ "target_id": created["id"] }))
            .await
            .unwrap();
        assert_eq!(closed["closed"], true);
    }
}
