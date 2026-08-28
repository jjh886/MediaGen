use std::sync::Arc;

use serde_json::{json, Value};

use crate::cdp::CdpClient;
use crate::dom_scripts::{
    download_script, element_point_script, focus_script, point_from_value, runtime_result_to_value,
    text_point_script,
};
use crate::rendering::{apply_viewport, normalize_device_scale_factor, restart_screencast};
use crate::{
    BrowserClipboardAction, BrowserClipboardRequest, BrowserClipboardResponse, BrowserGateway,
    BrowserGatewayError, BrowserHistoryAction, BrowserHistoryRequest, BrowserHistoryResponse,
    BrowserNavigateRequest, BrowserNavigateResponse, BrowserResizeRequest, BrowserResizeResponse,
    BrowserViewport,
};

impl BrowserGateway {
    pub async fn navigate_session(
        &self,
        id: &str,
        request: BrowserNavigateRequest,
    ) -> Result<BrowserNavigateResponse, BrowserGatewayError> {
        let url = normalize_url(&request.url)?;
        let client = self.session_client(id).await?;
        client
            .send("Page.navigate", json!({ "url": url.clone() }))
            .await?;
        self.update_session(id, |session| {
            session.url = url.clone();
            session.message = "Browser session 正在跳转页面".to_string();
            session.updated_at = chrono::Utc::now();
        })?;
        Ok(BrowserNavigateResponse { ok: true, url })
    }

    pub async fn history_session(
        &self,
        id: &str,
        request: BrowserHistoryRequest,
    ) -> Result<BrowserHistoryResponse, BrowserGatewayError> {
        let client = self.session_client(id).await?;
        match request.action {
            BrowserHistoryAction::Back => navigate_history(&client, -1).await?,
            BrowserHistoryAction::Forward => navigate_history(&client, 1).await?,
            BrowserHistoryAction::Reload => {
                client
                    .send("Page.reload", json!({ "ignoreCache": false }))
                    .await?;
            }
        }
        self.update_session(id, |session| {
            session.message = browser_history_message(request.action).to_string();
            session.updated_at = chrono::Utc::now();
        })?;
        Ok(BrowserHistoryResponse {
            ok: true,
            action: request.action,
        })
    }

    pub async fn resize_session(
        &self,
        id: &str,
        request: BrowserResizeRequest,
    ) -> Result<BrowserResizeResponse, BrowserGatewayError> {
        let viewport = BrowserViewport {
            width: request.width,
            height: request.height,
        }
        .clamped();
        let device_scale_factor = normalize_device_scale_factor(request.device_scale_factor);
        let client = self.session_client(id).await?;
        apply_viewport(&client, viewport, device_scale_factor).await?;
        restart_screencast(&client).await?;
        self.update_session(id, |session| {
            session.viewport = viewport;
            session.device_scale_factor = device_scale_factor;
            session.updated_at = chrono::Utc::now();
            session.message = "Browser session 视口尺寸已更新".to_string();
        })?;
        Ok(BrowserResizeResponse { ok: true, viewport })
    }

    pub async fn evaluate_script(
        &self,
        id: &str,
        script: &str,
        await_promise: bool,
    ) -> Result<Value, BrowserGatewayError> {
        self.evaluate_script_with_timeout(id, script, await_promise, None)
            .await
    }

    pub async fn evaluate_script_with_timeout(
        &self,
        id: &str,
        script: &str,
        await_promise: bool,
        timeout_ms: Option<u64>,
    ) -> Result<Value, BrowserGatewayError> {
        let mut retry = 0usize;
        loop {
            // 每次重试重新读取 client，目标被页面替换时可使用 watcher 刚重连的新 client。
            let client = self.session_client(id).await?;
            let result = client
                .send_with_timeout(
                    "Runtime.evaluate",
                    json!({
                        "expression": script,
                        "awaitPromise": await_promise,
                        "returnByValue": true,
                    }),
                    std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000).max(1)),
                )
                .await;
            match result {
                Ok(value) => return runtime_result_to_value(&value),
                Err(error) if retry < 2 && is_transient_navigation_error(&error) => {
                    let delay_ms = [400, 900][retry];
                    retry += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub async fn click_selector(
        &self,
        id: &str,
        selector: &str,
    ) -> Result<Value, BrowserGatewayError> {
        let point = self
            .evaluate_script(id, &element_point_script(selector), false)
            .await?;
        let Some((x, y)) = point_from_value(&point) else {
            return Ok(json!({ "clicked": false, "reason": "not_found" }));
        };
        self.dispatch_mouse_click(id, x, y).await?;
        Ok(json!({ "clicked": true, "x": x, "y": y }))
    }

    pub async fn click_text(
        &self,
        id: &str,
        text: &str,
        tag: &str,
        exact: bool,
    ) -> Result<Value, BrowserGatewayError> {
        let point = self
            .evaluate_script(id, &text_point_script(text, tag, exact), false)
            .await?;
        let Some((x, y)) = point_from_value(&point) else {
            return Ok(json!({ "clicked": false, "reason": "not_found" }));
        };
        self.dispatch_mouse_click(id, x, y).await?;
        Ok(json!({ "clicked": true, "x": x, "y": y }))
    }

    pub async fn type_text(
        &self,
        id: &str,
        selector: &str,
        text: &str,
        clear_first: bool,
    ) -> Result<Value, BrowserGatewayError> {
        let focused = self
            .evaluate_script(id, &focus_script(selector, clear_first), false)
            .await?;
        if focused.as_bool() != Some(true) {
            return Ok(json!({ "typed": false, "reason": "not_found" }));
        }
        self.session_client(id)
            .await?
            .send("Input.insertText", json!({ "text": text }))
            .await?;
        Ok(json!({ "typed": true }))
    }

    pub async fn set_file_input(
        &self,
        id: &str,
        selector: &str,
        files: Vec<String>,
    ) -> Result<Value, BrowserGatewayError> {
        let client = self.session_client(id).await?;
        let document = client.send("DOM.getDocument", json!({})).await?;
        let root_id = document["root"]["nodeId"]
            .as_i64()
            .ok_or_else(|| BrowserGatewayError::Cdp("DOM root nodeId 缺失".to_string()))?;
        let node = client
            .send(
                "DOM.querySelector",
                json!({ "nodeId": root_id, "selector": selector }),
            )
            .await?;
        let node_id = node["nodeId"].as_i64().unwrap_or_default();
        if node_id <= 0 {
            return Ok(json!({ "uploaded": false, "reason": "not_found" }));
        }
        client
            .send(
                "DOM.setFileInputFiles",
                json!({ "nodeId": node_id, "files": files }),
            )
            .await?;
        Ok(json!({ "uploaded": true }))
    }

    pub async fn download_in_browser(
        &self,
        id: &str,
        url: &str,
    ) -> Result<Value, BrowserGatewayError> {
        self.evaluate_script(id, &download_script(url), true).await
    }

    pub async fn clipboard(
        &self,
        id: &str,
        request: BrowserClipboardRequest,
    ) -> Result<BrowserClipboardResponse, BrowserGatewayError> {
        let client = self.session_client(id).await?;
        let text = match request.action {
            BrowserClipboardAction::Read => self.read_clipboard(id)?,
            BrowserClipboardAction::Write => {
                let text = request.text.unwrap_or_default();
                self.write_clipboard_state(id, text.clone())?;
                text
            }
            BrowserClipboardAction::InsertText => {
                let text = request.text.unwrap_or_default();
                client
                    .send("Input.insertText", json!({ "text": text }))
                    .await?;
                self.write_clipboard_state(id, text.clone())?;
                text
            }
            BrowserClipboardAction::CopySelection => self.read_remote_selection(&client).await?,
            BrowserClipboardAction::CutSelection => {
                let text = self.read_remote_selection(&client).await?;
                dispatch_edit_command(&client, "cut").await?;
                text
            }
            BrowserClipboardAction::SelectAll => {
                dispatch_edit_command(&client, "selectAll").await?;
                String::new()
            }
        };

        self.push_event(
            id,
            "clipboard",
            json!({ "session_id": id, "action": request.action }),
        )?;
        Ok(BrowserClipboardResponse { ok: true, text })
    }

    async fn dispatch_mouse_click(
        &self,
        id: &str,
        x: f64,
        y: f64,
    ) -> Result<(), BrowserGatewayError> {
        let client = self.session_client(id).await?;
        for (kind, button, buttons) in [
            ("mouseMoved", "none", 0),
            ("mousePressed", "left", 1),
            ("mouseReleased", "left", 0),
        ] {
            client
                .send(
                    "Input.dispatchMouseEvent",
                    json!({
                        "type": kind,
                        "x": x,
                        "y": y,
                        "button": button,
                        "buttons": buttons,
                        "clickCount": 1,
                    }),
                )
                .await?;
        }
        Ok(())
    }

    async fn read_remote_selection(
        &self,
        client: &Arc<CdpClient>,
    ) -> Result<String, BrowserGatewayError> {
        let expression = r#"
(() => {
  const active = document.activeElement;
  if (active && ['INPUT', 'TEXTAREA'].includes(active.tagName)) {
    if (String(active.type || '').toLowerCase() === 'password') return '';
    const start = Number(active.selectionStart);
    const end = Number(active.selectionEnd);
    if (Number.isFinite(start) && Number.isFinite(end) && end > start) {
      return String(active.value || '').slice(start, end);
    }
  }
  return String(window.getSelection()?.toString() || '');
})()
"#;
        let result = client
            .send(
                "Runtime.evaluate",
                json!({ "expression": expression, "returnByValue": true }),
            )
            .await?;
        Ok(read_runtime_value(&result))
    }
}

async fn dispatch_edit_command(
    client: &Arc<CdpClient>,
    command: &str,
) -> Result<(), BrowserGatewayError> {
    client
        .send(
            "Input.dispatchKeyEvent",
            json!({ "type": "rawKeyDown", "commands": [command] }),
        )
        .await?;
    Ok(())
}

pub(crate) fn read_runtime_value(result: &Value) -> String {
    result
        .get("result")
        .and_then(|value| value.get("value"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

async fn navigate_history(client: &CdpClient, offset: i64) -> Result<(), BrowserGatewayError> {
    let history = client.send("Page.getNavigationHistory", json!({})).await?;
    let current = history["currentIndex"].as_i64().unwrap_or_default();
    let target = current + offset;
    let entries = history["entries"]
        .as_array()
        .ok_or_else(|| BrowserGatewayError::Cdp("浏览器历史记录为空".to_string()))?;
    let entry_id = entries
        .get(target as usize)
        .and_then(|entry| entry.get("id"))
        .and_then(Value::as_i64)
        .ok_or_else(|| BrowserGatewayError::Cdp("没有可跳转的浏览器历史记录".to_string()))?;
    client
        .send(
            "Page.navigateToHistoryEntry",
            json!({ "entryId": entry_id }),
        )
        .await?;
    Ok(())
}

fn browser_history_message(action: BrowserHistoryAction) -> &'static str {
    match action {
        BrowserHistoryAction::Back => "Browser session 正在后退",
        BrowserHistoryAction::Forward => "Browser session 正在前进",
        BrowserHistoryAction::Reload => "Browser session 正在刷新",
    }
}

fn normalize_url(raw: &str) -> Result<String, BrowserGatewayError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(BrowserGatewayError::Cdp("跳转地址不能为空".to_string()));
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return Ok(value.to_string());
    }
    Ok(format!("https://{value}"))
}

fn is_transient_navigation_error(error: &BrowserGatewayError) -> bool {
    let BrowserGatewayError::Cdp(message) = error else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    [
        "inspected target navigated or closed",
        "execution context was destroyed",
        "cannot find context with specified id",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::is_transient_navigation_error;
    use crate::BrowserGatewayError;

    #[test]
    fn navigation_race_is_retryable_but_normal_script_errors_are_not() {
        assert!(is_transient_navigation_error(&BrowserGatewayError::Cdp(
            r#"{"code":-32000,"message":"Inspected target navigated or closed"}"#.to_string()
        )));
        assert!(is_transient_navigation_error(&BrowserGatewayError::Cdp(
            "Execution context was destroyed".to_string()
        )));
        assert!(!is_transient_navigation_error(&BrowserGatewayError::Cdp(
            "ReferenceError: missing is not defined".to_string()
        )));
    }
}
