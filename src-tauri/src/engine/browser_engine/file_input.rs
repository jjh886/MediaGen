//! browser.set_file 的参数解析与执行。
//!
//! 单独成模块是因为"向 `<input type=file>` 注入多文件"这块独立于鼠标/键盘交互，
//! 而且需要兼容两种参数形态（模板字符串 vs 原生数组）。

use serde_json::json;
use tracing::info;

use crate::error::AppError;
use crate::types::StepResult;

/// 从 set_file 的 params 中解析文件路径列表。
/// 优先级：`file_paths`（数组 / JSON 字符串形式数组） > `file_path`（单路径字符串）。
/// 模板引擎会把 context 里的 Vec<String> stringify 成 JSON 字符串，这里兼容两种形态。
pub(crate) fn collect_file_paths(params: &serde_json::Value) -> Vec<String> {
    if let Some(arr) = params.get("file_paths").and_then(|v| v.as_array()) {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(s) = params.get("file_paths").and_then(|v| v.as_str()) {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if let Ok(list) = serde_json::from_str::<Vec<String>>(trimmed) {
            return list.into_iter().filter(|p| !p.is_empty()).collect();
        }
    }
    if let Some(p) = params.get("file_path").and_then(|v| v.as_str()) {
        if !p.is_empty() {
            return vec![p.to_string()];
        }
    }
    Vec::new()
}

impl super::BrowserEngine {
    /// browser.set_file: 给 file input 设置文件路径（用于参考图上传，支持多文件）
    ///
    /// 通过 CDP DOM.setFileInputFiles 一次设置一个或多个文件，不触发文件选择弹窗。
    /// 参数优先级：`file_paths`（数组或 JSON 字符串形式的数组） > `file_path`（单文件）。
    /// 路径为空或文件不存在时跳过。
    pub(crate) async fn set_file(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let session_id = params["session_id"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.set_file 缺少 session_id".into()))?;
        let selector = params["selector"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.set_file 缺少 selector".into()))?;

        let file_paths: Vec<String> = collect_file_paths(params);

        if file_paths.is_empty() {
            return Ok(StepResult {
                success: true,
                data: json!({ "skipped": true, "reason": "no file_path" }),
                error: None,
            });
        }

        // 过滤掉不存在的文件；全部不存在时视为跳过
        let existing: Vec<String> = file_paths
            .into_iter()
            .filter(|p| std::path::Path::new(p).exists())
            .collect();
        if existing.is_empty() {
            return Ok(StepResult {
                success: true,
                data: json!({ "skipped": true, "reason": "file not found" }),
                error: None,
            });
        }

        let tab = self.get_tab(session_id)?;
        let selector = selector.to_string();
        let paths_for_task = existing.clone();

        let result = tokio::task::spawn_blocking(move || {
            let timeout = std::time::Duration::from_millis(5000);
            let el = tab
                .wait_for_element_with_custom_timeout(&selector, timeout)
                .map_err(|e| {
                    AppError::BrowserError(format!("找不到 file input '{selector}': {e}"))
                })?;
            let refs: Vec<&str> = paths_for_task.iter().map(String::as_str).collect();
            el.set_input_files(&refs)
                .map_err(|e| AppError::BrowserError(format!("设置文件失败: {e}")))?;
            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::BrowserError(format!("set_file 任务失败: {e}")))?;

        result?;
        info!(count = existing.len(), "文件已设置到 input");

        Ok(StepResult {
            success: true,
            data: json!({ "uploaded": true, "count": existing.len() }),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::collect_file_paths;
    use serde_json::json;

    #[test]
    fn file_paths_json_array_field_is_parsed() {
        let params = json!({ "file_paths": ["/tmp/a.png", "/tmp/b.jpg"] });
        assert_eq!(
            collect_file_paths(&params),
            vec!["/tmp/a.png", "/tmp/b.jpg"]
        );
    }

    #[test]
    fn file_paths_json_string_from_template_engine_is_parsed() {
        // orchestrator 把 context 里的 Vec<String> 用 .to_string() 序列化成 JSON 字符串
        let params = json!({ "file_paths": "[\"/tmp/a.png\",\"/tmp/b.jpg\"]" });
        assert_eq!(
            collect_file_paths(&params),
            vec!["/tmp/a.png", "/tmp/b.jpg"]
        );
    }

    #[test]
    fn file_path_single_string_falls_back() {
        let params = json!({ "file_path": "/tmp/only.png" });
        assert_eq!(collect_file_paths(&params), vec!["/tmp/only.png"]);
    }

    #[test]
    fn empty_inputs_return_empty() {
        assert!(collect_file_paths(&json!({})).is_empty());
        assert!(collect_file_paths(&json!({ "file_path": "" })).is_empty());
        assert!(collect_file_paths(&json!({ "file_paths": "" })).is_empty());
        assert!(collect_file_paths(&json!({ "file_paths": "[]" })).is_empty());
        assert!(collect_file_paths(&json!({ "file_paths": [] })).is_empty());
    }

    #[test]
    fn file_paths_takes_priority_over_file_path() {
        let params = json!({
            "file_paths": ["/tmp/a.png"],
            "file_path": "/tmp/ignored.png",
        });
        assert_eq!(collect_file_paths(&params), vec!["/tmp/a.png"]);
    }
}
