use serde_json::json;
use tracing::{debug, info};

use super::evaluate_poll_condition;
use crate::engine::retry::{is_transient_network_error, sleep_blocking, RetryConfig};
use crate::error::AppError;
use crate::types::StepResult;

fn jitter_ms(base_ms: u64, pct: u64) -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let range = base_ms * pct / 100;
    if range == 0 {
        return base_ms;
    }
    let offset = nanos % (range * 2);
    // base_ms ± pct%
    let result = base_ms + offset - range;
    result.max(500)
}

impl super::BrowserEngine {
    /// browser.navigate: 导航到指定 URL（空 URL 跳过）
    pub(crate) async fn navigate(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let url = params["url"].as_str().unwrap_or("").to_string();

        // 空 URL 或无效 URL 时跳过导航
        if url.is_empty() || !url.starts_with("http") {
            debug!(url = %url, "URL 为空或无效，跳过导航");
            return Ok(StepResult {
                success: true,
                data: json!({ "url": url, "skipped": true }),
                error: None,
            });
        }
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.navigate 缺少 session_id".into()))?;
        let wait_ms = params["wait_ms"].as_u64().unwrap_or(5000);
        let retry = RetryConfig::from_params(params, 3, &[2_000, 5_000, 10_000]);

        let tab = self.get_tab(session_id)?;
        let url_clone = url.clone();

        let title = tokio::task::spawn_blocking(move || {
            for attempt in 1..=retry.attempts() {
                match tab.navigate_to(&url_clone) {
                    Ok(_) => break,
                    Err(e) => {
                        let error = e.to_string();
                        if attempt == retry.attempts() || !is_transient_network_error(&error) {
                            return Err(AppError::BrowserError(format!("导航失败: {error}")));
                        }
                        sleep_blocking(&retry, attempt);
                    }
                }
            }

            // navigate_to 只保证当前 CDP target 完成导航，不保证这个 target
            // 一定是 Chrome UI 里肉眼可见的激活页。调试模式 headless=false 时，
            // 用户需要看到同一个窗口的前台 tab 跳到目标站点，所以每次导航后
            // 都重新激活并前置受控 tab。
            if let Err(e) = tab.activate() {
                debug!(error = %e, "导航后激活受控 tab 失败，继续执行");
            }
            if let Err(e) = tab.bring_to_front() {
                debug!(error = %e, "导航后前置受控 tab 失败，继续执行");
            }

            // 添加 ±20% 的随机抖动，避免固定时序被检测
            std::thread::sleep(std::time::Duration::from_millis(jitter_ms(wait_ms, 20)));

            let title = tab
                .evaluate("document.title", false)
                .ok()
                .and_then(|r| r.value)
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            Ok::<String, AppError>(title)
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("导航任务失败: {e}")))?
        .map_err(|e| e)?;

        debug!(url = %url, title = %title, "导航完成");

        Ok(StepResult {
            success: true,
            data: json!({ "url": url, "title": title }),
            error: None,
        })
    }

    /// browser.run_script: 在当前页面执行 JS 脚本
    pub(crate) async fn run_script(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.run_script 缺少 session_id".into()))?;
        let script = params["script"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.run_script 缺少 script".into()))?
            .to_string();
        let await_promise = params["await_promise"].as_bool().unwrap_or(false);

        // 诊断上下文：脚本失败时（常见 -32000 target navigated/closed）会和当前 URL/title 一起打印
        let session_id_owned = session_id.to_string();
        let save_as = params["save_as"].as_str().unwrap_or("").to_string();
        let script_file = params["script_file"].as_str().unwrap_or("").to_string();
        let script_preview: String = script.chars().take(120).collect();

        let tab = self.get_tab(session_id)?;

        let data = tokio::task::spawn_blocking(move || {
            let result = tab.evaluate(&script, await_promise).map_err(|e| {
                // target 可能已关闭，尽力取 URL/title；取失败就留空
                let current_url = tab.get_url();
                let current_title = tab
                    .evaluate("document.title", false)
                    .ok()
                    .and_then(|r| r.value)
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default();
                tracing::warn!(
                    session_id = %session_id_owned,
                    save_as = %save_as,
                    script_file = %script_file,
                    script_preview = %script_preview,
                    current_url = %current_url,
                    current_title = %current_title,
                    error = %e,
                    "run_script 失败：target 可能已导航或关闭"
                );
                AppError::BrowserError(format!(
                    "脚本执行失败: {e} (url={current_url}, title={current_title})"
                ))
            })?;

            let data = result
                .value
                .as_ref()
                .and_then(|v| {
                    v.as_str()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                        .or_else(|| Some(v.clone()))
                })
                .unwrap_or(json!(null));
            Ok::<serde_json::Value, AppError>(data)
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("脚本任务失败: {e}")))?
        .map_err(|e| e)?;

        // 检查脚本返回值中是否包含 error 字段
        let has_error = data
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);

        if has_error {
            let err_msg = data["error"].as_str().unwrap_or("unknown").to_string();
            tracing::warn!(error = %err_msg, data = %data, "脚本返回错误");
            return Ok(StepResult {
                success: false,
                data,
                error: Some(err_msg),
            });
        }

        debug!(data = %data, "脚本执行完成");

        Ok(StepResult {
            success: true,
            data,
            error: None,
        })
    }
    /// browser.poll_script: 定期执行 JS 脚本直到条件满足（类似 http.poll）
    ///
    /// 用于浏览器内长时间异步操作（如生成任务），脚本应返回 JSON 字符串。
    /// 参数：session_id, script, until (条件表达式), interval_ms, timeout_ms, await_promise
    pub(crate) async fn poll_script(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.poll_script 缺少 session_id".into()))?;
        let script = params["script"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.poll_script 缺少 script".into()))?;
        let until = params["until"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.poll_script 缺少 until 条件".into()))?
            .to_string();
        let interval_ms = params["interval_ms"].as_u64().unwrap_or(3_000);
        let timeout_ms = params["timeout_ms"].as_u64().unwrap_or(600_000);
        let await_promise = params["await_promise"].as_bool().unwrap_or(false);

        let tab = self.get_tab(session_id)?;
        let script = script.to_string();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

        loop {
            let tab_clone = tab.clone();
            let script_clone = script.clone();
            let eval_result = tokio::task::spawn_blocking(move || {
                tab_clone.evaluate(&script_clone, await_promise)
            })
            .await
            .map_err(|e| AppError::BrowserError(format!("脚本执行任务失败: {e}")))?;

            match eval_result {
                Ok(result) => {
                    let data = result
                        .value
                        .as_ref()
                        .and_then(|v| {
                            v.as_str()
                                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                                .or_else(|| Some(v.clone()))
                        })
                        .unwrap_or(json!(null));

                    debug!(data = %data, "browser.poll_script 轮询中");

                    // 检查完成条件
                    if evaluate_poll_condition(&data, &until) {
                        info!(data = %data, "browser.poll_script 条件满足");
                        return Ok(StepResult {
                            success: true,
                            data,
                            error: None,
                        });
                    }

                    // 检查错误状态：error / needs_verification 都视为失败
                    let status_str = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if status_str == "error" || status_str == "needs_verification" {
                        let err_msg = data
                            .get("error")
                            .or(data.get("message"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown error")
                            .to_string();
                        return Ok(StepResult {
                            success: false,
                            data,
                            error: Some(err_msg),
                        });
                    }

                    // 兼容 { error: "msg" } 格式
                    let has_error = data
                        .get("error")
                        .and_then(|v| v.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                    if has_error {
                        let err_msg = data["error"]
                            .as_str()
                            .unwrap_or("unknown error")
                            .to_string();
                        return Ok(StepResult {
                            success: false,
                            data,
                            error: Some(err_msg),
                        });
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    // CDP 连接已断（Chrome 被 kill / 崩溃 / 正常关闭），tab 不可恢复：
                    // 立即失败退出，避免继续重试把任务拖到 timeout_ms（通常 10 分钟）。
                    if msg.contains("underlying connection is closed")
                        || msg.contains("tab closed")
                        || msg.contains("session closed")
                    {
                        tracing::warn!(error = %msg, "browser.poll_script 检测到 CDP 连接已断，立即退出");
                        return Ok(StepResult {
                            success: false,
                            data: json!({"status": "error", "reason": "browser_disconnected"}),
                            error: Some(format!("浏览器连接已断开: {msg}")),
                        });
                    }
                    debug!(error = %msg, "browser.poll_script 脚本执行出错，继续重试");
                }
            }

            if std::time::Instant::now() >= deadline {
                return Ok(StepResult {
                    success: false,
                    data: json!({"status": "timeout"}),
                    error: Some(format!("browser.poll_script 超时 ({timeout_ms}ms)")),
                });
            }

            // 轮询间隔添加 ±30% 抖动
            tokio::time::sleep(std::time::Duration::from_millis(jitter_ms(interval_ms, 30))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::retry::is_transient_network_error;

    #[test]
    fn recognizes_transient_navigation_errors_from_proxy_disconnects() {
        assert!(is_transient_network_error(
            "Navigate failed: net::ERR_CONNECTION_CLOSED"
        ));
        assert!(is_transient_network_error(
            "Navigate failed: net::ERR_TUNNEL_CONNECTION_FAILED"
        ));
    }

    #[test]
    fn does_not_retry_non_network_navigation_errors() {
        assert!(!is_transient_network_error(
            "Cannot navigate to invalid URL"
        ));
    }
}
