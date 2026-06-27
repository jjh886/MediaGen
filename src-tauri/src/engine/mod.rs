pub mod account_engine;
pub mod browser_engine;
pub mod callback_server_engine;
pub(crate) mod condition;
pub mod crypto_engine;
pub mod file_engine;
pub mod http_engine;
pub mod image_engine;
pub mod process_engine;
pub(crate) mod retry;
pub mod watermark_engine;

use async_trait::async_trait;
use std::sync::Arc;

use crate::error::AppError;
use crate::types::StepResult;

#[async_trait]
pub trait AtomExecutor: Send + Sync {
    async fn execute(
        &self,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError>;

    /// 是否处理该 action 前缀（如 "http"、"file"）
    fn handles(&self, action_prefix: &str) -> bool;
}

pub struct EngineRegistry {
    engines: Vec<Arc<dyn AtomExecutor>>,
}

impl EngineRegistry {
    pub fn new() -> Self {
        Self {
            engines: Vec::new(),
        }
    }

    pub fn register(&mut self, engine: Box<dyn AtomExecutor>) {
        self.engines.push(Arc::from(engine));
    }

    pub fn cloned(&self) -> Self {
        Self {
            engines: self.engines.clone(),
        }
    }

    pub async fn execute(
        &self,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let prefix = action.split('.').next().unwrap_or(action);
        for engine in &self.engines {
            if engine.handles(prefix) {
                return engine.execute(action, params).await;
            }
        }
        Err(AppError::PlatformError(format!(
            "没有找到处理 '{}' 的引擎",
            action
        )))
    }
}
