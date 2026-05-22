use async_trait::async_trait;

use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::types::StepResult;

/// 图片处理引擎：裁剪、缩放、格式转换等通用图片操作
/// 当前预留，待有实际需求时实现具体操作
pub struct ImageEngine;

impl ImageEngine {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AtomExecutor for ImageEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "image"
    }

    async fn execute(
        &self,
        action: &str,
        _params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        Err(AppError::PlatformError(format!(
            "image 引擎尚未实现: {action}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_prefix() {
        let engine = ImageEngine::new();
        assert!(engine.handles("image"));
        assert!(!engine.handles("watermark"));
    }
}
