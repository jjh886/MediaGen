use std::sync::Arc;

use mediagen_browser_gateway::{BrowserGateway, BrowserSessionStatus};
use mediagen_core::PluginManifest;
use serde_json::Value;

/// 记录一次 flow 实际打开的浏览器，确保正常结束、提前失败和取消都能统一回收。
pub(crate) struct FlowBrowserLifecycle {
    browser_gateway: Arc<BrowserGateway>,
    session_ids: Vec<String>,
    persistent: bool,
}

impl FlowBrowserLifecycle {
    pub(crate) fn new(browser_gateway: Arc<BrowserGateway>, persistent: bool) -> Self {
        Self {
            browser_gateway,
            session_ids: Vec::new(),
            persistent,
        }
    }

    pub(crate) fn remember_open(&mut self, action: &str, result: &Value) {
        if action != "browser.open" {
            return;
        }
        let Some(session_id) = result.get("session_id").and_then(Value::as_str) else {
            return;
        };
        if !self.session_ids.iter().any(|item| item == session_id) {
            self.session_ids.push(session_id.to_string());
        }
    }

    /// 失败时只取消本任务 Tab；成功时按插件策略释放常驻 Tab 或结束临时 Tab。
    pub(crate) async fn finish(&self, succeeded: bool) {
        for session_id in &self.session_ids {
            let Ok(session) = self.browser_gateway.get_session(session_id) else {
                continue;
            };
            if session.status != BrowserSessionStatus::Active {
                continue;
            }
            let result = if !succeeded {
                self.browser_gateway.cancel_session(session_id).await
            } else if self.persistent {
                self.browser_gateway.release_session(session_id).await
            } else {
                self.browser_gateway.complete_session(session_id).await
            };
            if let Err(error) = result {
                eprintln!("[mediagen-server] 清理浏览器会话失败 session_id={session_id}: {error}");
            }
        }
    }
}

pub(crate) fn is_persistent_browser(manifest: &PluginManifest) -> bool {
    manifest
        .extra
        .get("browser_lifecycle")
        .and_then(Value::as_str)
        == Some("persistent")
}
