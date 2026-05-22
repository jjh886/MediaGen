use async_trait::async_trait;
use base64::prelude::{Engine as _, BASE64_STANDARD};
use serde_json::json;

use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::types::StepResult;

const MAX_DATA_URL_FILE_BYTES: u64 = 50 * 1024 * 1024;

pub struct FileEngine;

impl FileEngine {
    pub fn new() -> Self {
        Self
    }

    async fn write_json(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let path = params["path"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("file.write_json 缺少 path 参数".into()))?;
        let value = resolve_write_json_value(params)?;
        let path = std::path::PathBuf::from(path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        let content = serde_json::to_vec_pretty(&value)?;
        tokio::fs::write(&tmp, [&content[..], b"\n"].concat()).await?;
        tokio::fs::rename(&tmp, &path).await?;

        Ok(StepResult {
            success: true,
            data: json!({ "path": path.to_string_lossy(), "bytes": content.len() + 1 }),
            error: None,
        })
    }

    async fn read_json(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let path = params["path"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("file.read_json 缺少 path 参数".into()))?;
        let content = tokio::fs::read_to_string(path).await?;
        let data: serde_json::Value = serde_json::from_str(&content)?;
        Ok(StepResult {
            success: true,
            data,
            error: None,
        })
    }

    async fn read_base64(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let path = params["path"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("file.read_base64 缺少 path 参数".into()))?;
        let max_bytes = params["max_bytes"]
            .as_u64()
            .unwrap_or(MAX_DATA_URL_FILE_BYTES);
        let metadata = tokio::fs::metadata(path).await.map_err(|e| {
            AppError::PlatformError(format!("file.read_base64 找不到文件 {path}: {e}"))
        })?;
        if metadata.len() > max_bytes {
            return Err(AppError::PlatformError(format!(
                "file.read_base64 文件超过限制 {max_bytes} bytes: {path}"
            )));
        }
        let bytes = tokio::fs::read(path).await.map_err(|e| {
            AppError::PlatformError(format!("file.read_base64 读取失败 {path}: {e}"))
        })?;
        let mime = mime_type(path);
        Ok(StepResult {
            success: true,
            data: json!({
                "path": path,
                "base64": BASE64_STANDARD.encode(&bytes),
                "mime": mime,
                "size": bytes.len() as u64,
            }),
            error: None,
        })
    }
}

#[async_trait]
impl AtomExecutor for FileEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "file"
    }

    async fn execute(
        &self,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        match action {
            "file.write_json" => self.write_json(params).await,
            "file.read_json" => self.read_json(params).await,
            "file.read_base64" => self.read_base64(params).await,
            _ => Err(AppError::PlatformError(format!(
                "未知的 file 操作: {action}，可用操作: file.write_json, file.read_json, file.read_base64"
            ))),
        }
    }
}

fn mime_type(path: &str) -> &'static str {
    match std::path::Path::new(path)
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

fn resolve_write_json_value(params: &serde_json::Value) -> Result<serde_json::Value, AppError> {
    if let Some(value) = params.get("value") {
        return Ok(value.clone());
    }
    let context = params.get("context").unwrap_or(params);
    let value_path = params["value_path"].as_str().ok_or_else(|| {
        AppError::PlatformError("file.write_json 缺少 value 或 value_path".into())
    })?;
    lookup_dot_path(context, value_path)
        .cloned()
        .ok_or_else(|| {
            AppError::PlatformError(format!("file.write_json 找不到 value_path: {value_path}"))
        })
}

fn lookup_dot_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_only_file_prefix() {
        let engine = FileEngine::new();
        assert!(engine.handles("file"));
        assert!(!engine.handles("http"));
    }

    #[tokio::test]
    async fn write_json_writes_context_value_atomically() {
        let engine = FileEngine::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");

        let result = engine
            .execute(
                "file.write_json",
                &json!({
                    "path": path,
                    "context": {
                        "auth_file": {
                            "tokens": { "access_token": "access" }
                        }
                    },
                    "value_path": "auth_file"
                }),
            )
            .await
            .unwrap();

        assert!(result.success);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved["tokens"]["access_token"], "access");
    }

    #[tokio::test]
    async fn read_base64_returns_raw_file_data_without_data_url() {
        let engine = FileEngine::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ref.webp");
        std::fs::write(&path, b"image-bytes").unwrap();

        let result = engine
            .execute(
                "file.read_base64",
                &json!({
                    "path": path,
                    "max_bytes": 1024
                }),
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.data["base64"], "aW1hZ2UtYnl0ZXM=");
        assert_eq!(result.data["mime"], "image/webp");
    }
}
