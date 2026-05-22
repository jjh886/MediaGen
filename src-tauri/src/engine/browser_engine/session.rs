//! session 级浏览器交互操作：cookies 读/写、浏览器内下载、窗口显示。
//! `browser.close` 的释放/关闭逻辑（含并发安全语义）在同目录的 `close.rs`。

use headless_chrome::protocol::cdp::Network;
use serde_json::json;
use tracing::info;

use super::cookie::json_to_cookie_param;
use crate::error::AppError;
use crate::types::StepResult;

impl super::BrowserEngine {
    /// browser.get_cookies: 获取当前页面的 cookies（通过 CDP 协议，含 httpOnly）
    pub(crate) async fn get_cookies(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.get_cookies 缺少 session_id".into()))?;

        let tab = self.get_tab(session_id)?;
        let cookie_list = tokio::task::spawn_blocking(move || {
            let cookies = tab
                .get_cookies()
                .map_err(|e| AppError::BrowserError(format!("获取 cookies 失败: {e}")))?;
            let list: Vec<serde_json::Value> = cookies
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name,
                        "value": c.value,
                        "domain": c.domain,
                        "path": c.path,
                        "secure": c.secure,
                        "httpOnly": c.http_only,
                    })
                })
                .collect();
            Ok::<Vec<serde_json::Value>, AppError>(list)
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("get_cookies 任务失败: {e}")))?
        .map_err(|e| e)?;

        Ok(StepResult {
            success: true,
            data: json!({ "cookies": cookie_list }),
            error: None,
        })
    }
    /// browser.download: 在浏览器内下载文件（自带 cookie，解决需要认证的下载）
    ///
    /// 用 fetch + blob 在浏览器 JS 中下载，然后通过 CDP 把数据传回 Rust 保存为本地文件。
    pub(crate) async fn browser_download(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.download 缺少 session_id".into()))?;
        let url = params["url"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.download 缺少 url".into()))?
            .to_string();

        let tab = self.get_tab(session_id)?;
        let url_clone = url.clone();

        let (base64_data, content_type) = tokio::task::spawn_blocking(move || {
            // 在浏览器中 fetch 并转为 base64
            let script = format!(r#"
                (async () => {{
                    try {{
                        const resp = await fetch("{}", {{ credentials: 'include' }});
                        if (!resp.ok) return JSON.stringify({{ error: 'HTTP ' + resp.status }});
                        const blob = await resp.blob();
                        const buf = await blob.arrayBuffer();
                        const bytes = new Uint8Array(buf);
                        let binary = '';
                        for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
                        return JSON.stringify({{
                            data: btoa(binary),
                            type: blob.type,
                            size: bytes.length,
                        }});
                    }} catch (e) {{
                        return JSON.stringify({{ error: e.message }});
                    }}
                }})()
            "#, url_clone.replace('\\', "\\\\").replace('"', "\\\""));

            let result = tab
                .evaluate(&script, true)
                .map_err(|e| AppError::BrowserError(format!("browser.download 脚本失败: {e}")))?;

            let val: serde_json::Value = result.value
                .as_ref()
                .and_then(|v| v.as_str().and_then(|s| serde_json::from_str(s).ok()).or_else(|| Some(v.clone())))
                .unwrap_or(serde_json::json!(null));

            if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
                return Err(AppError::BrowserError(format!("browser.download 失败: {err}")));
            }

            let data = val["data"].as_str().unwrap_or("").to_string();
            let ct = val["type"].as_str().unwrap_or("application/octet-stream").to_string();
            Ok::<(String, String), AppError>((data, ct))
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("download 任务失败: {e}")))?
        .map_err(|e| e)?;

        // 解码 base64 并保存到临时文件
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&base64_data)
            .map_err(|e| AppError::BrowserError(format!("Base64 解码失败: {e}")))?;

        let ext = if content_type.contains("png") {
            "png"
        } else if content_type.contains("jpeg") || content_type.contains("jpg") {
            "jpg"
        } else if content_type.contains("mp4") {
            "mp4"
        } else {
            "bin"
        };
        let unique_id = uuid::Uuid::new_v4().to_string();
        let tmp_path = std::env::temp_dir().join(format!("frame64_{unique_id}.{ext}"));
        std::fs::write(&tmp_path, &bytes)
            .map_err(|e| AppError::BrowserError(format!("保存文件失败: {e}")))?;

        info!(size = bytes.len(), path = %tmp_path.display(), "浏览器内下载完成");

        Ok(StepResult {
            success: true,
            data: json!({
                "path": tmp_path.to_string_lossy(),
                "size": bytes.len(),
                "content_type": content_type,
            }),
            error: None,
        })
    }

    /// browser.restore_cookies: 从 JSON 恢复 cookie（在 navigate 之后、交互之前调用）
    ///
    /// 解决 Chrome 关闭后 session cookie 丢失的问题：
    /// 启动 Chrome → navigate 到目标站点 → restore_cookies → 刷新页面
    pub(crate) async fn restore_cookies(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"].as_str().ok_or_else(|| {
            AppError::BrowserError("browser.restore_cookies 缺少 session_id".into())
        })?;
        let cookies_json = params["cookies_json"].as_str().ok_or_else(|| {
            AppError::BrowserError("browser.restore_cookies 缺少 cookies_json".into())
        })?;
        let reload = params["reload"].as_bool().unwrap_or(true);

        let tab = self.get_tab(session_id)?;

        // 解析 cookie JSON（格式与 get_cookies 返回的一致）
        let cookies: Vec<serde_json::Value> = serde_json::from_str(cookies_json)
            .map_err(|e| AppError::BrowserError(format!("Cookie JSON 解析失败: {e}")))?;

        // 与 inject_cookies_via_cdp / write_cookies_to_profile 共用同一个 JSON→CookieParam 转换：
        // 完整保留 sameSite/sourceScheme/sourcePort/partitionKey/priority，
        // 否则跨站 cookie 会被 Chrome 当成新 cookie 而不是替换既有的
        let cookie_params: Vec<Network::CookieParam> =
            cookies.iter().filter_map(json_to_cookie_param).collect();

        let count = cookie_params.len();
        let tab_clone = tab.clone();
        let reload_flag = reload;

        tokio::task::spawn_blocking(move || {
            tab_clone
                .set_cookies(cookie_params)
                .map_err(|e| AppError::BrowserError(format!("注入 cookie 失败: {e}")))?;
            if reload_flag {
                let _ = tab_clone.evaluate("location.reload()", false);
            }
            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("restore_cookies 任务失败: {e}")))?
        .map_err(|e| e)?;

        info!(count, "Cookie 已恢复");

        Ok(StepResult {
            success: true,
            data: json!({ "restored": count }),
            error: None,
        })
    }

    /// browser.show_window: 把浏览器窗口移到可见区域（供人工验证触发后拉回屏幕用）
    ///
    /// 默认位置 (100, 100)，尺寸 1400x900，可通过 `left/top/width/height` 覆盖。
    pub(crate) async fn show_window(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.show_window 缺少 session_id".into()))?
            .to_string();
        let left = params["left"].as_i64().unwrap_or(100) as u32;
        let top = params["top"].as_i64().unwrap_or(100) as u32;
        let width = params["width"].as_f64().unwrap_or(1400.0);
        let height = params["height"].as_f64().unwrap_or(900.0);

        let tab = self.get_tab(&session_id)?;
        let sid_for_task = session_id.clone();
        tokio::task::spawn_blocking(move || {
            tab.set_bounds(headless_chrome::types::Bounds::Normal {
                left: Some(left),
                top: Some(top),
                width: Some(width),
                height: Some(height),
            })
            .map_err(|e| AppError::BrowserError(format!("设置窗口位置失败: {e}")))?;
            info!(session_id = %sid_for_task, left, top, width, height, "浏览器窗口已移到可见区域");
            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("show_window 任务失败: {e}")))??;

        Ok(StepResult {
            success: true,
            data: json!({ "session_id": session_id, "left": left, "top": top, "width": width, "height": height }),
            error: None,
        })
    }
}
