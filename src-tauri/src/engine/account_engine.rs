use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use std::sync::Mutex;
use tracing::info;

use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::storage::Storage;
use crate::sync_ext::MutexExt;
use crate::types::{LoginState, StepResult};

pub struct AccountEngine {
    storage: Arc<Mutex<Storage>>,
}

impl AccountEngine {
    pub fn new(storage: Arc<Mutex<Storage>>) -> Self {
        Self { storage }
    }

    /// account.set_state: 根据条件设置账号登录状态
    async fn set_state(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let account_id = params["account_id"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("account.set_state 缺少 account_id".into()))?;
        let condition = params["condition"].as_bool().unwrap_or(false);
        let state_if_true = params["state_if_true"].as_str().unwrap_or("available");
        let state_if_false = params["state_if_false"].as_str().unwrap_or("login_expired");

        let state_str = if condition {
            state_if_true
        } else {
            state_if_false
        };
        let state: LoginState =
            serde_json::from_value(serde_json::Value::String(state_str.to_string()))
                .map_err(|_| AppError::PlatformError(format!("无效的登录状态: {state_str}")))?;

        let reason = if condition {
            "检测可用"
        } else {
            "检测不可用"
        };

        let storage = self.storage.lock_recover();
        storage.update_login_state(account_id, state.clone(), reason)?;
        drop(storage);

        info!(account_id, state = state_str, "账号状态已更新");

        Ok(StepResult {
            success: true,
            data: json!({ "account_id": account_id, "new_state": state_str }),
            error: None,
        })
    }

    /// account.save_credentials: 保存登录凭据到账号
    async fn save_credentials(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let account_id = params["account_id"].as_str().ok_or_else(|| {
            AppError::PlatformError("account.save_credentials 缺少 account_id".into())
        })?;

        let storage = self.storage.lock_recover();
        let mut account = storage
            .get_account(account_id)?
            .ok_or_else(|| AppError::PlatformError(format!("账号不存在: {account_id}")))?;

        // 合并凭据字段（非空才覆盖）
        if let Some(v) = params["cookies"].as_str().filter(|s| !s.is_empty()) {
            account.cookies = v.to_string();
        }
        if let Some(v) = params["api_token"].as_str().filter(|s| !s.is_empty()) {
            account.api_token = v.to_string();
        }
        if let Some(v) = params["refresh_token"].as_str().filter(|s| !s.is_empty()) {
            account.refresh_token = v.to_string();
        }
        if let Some(v) = params["user_id"].as_str().filter(|s| !s.is_empty()) {
            account.user_id = v.to_string();
        }
        if let Some(v) = params["username"].as_str().filter(|s| !s.is_empty()) {
            account.username = v.to_string();
            account.name = v.to_string();
        }
        if let Some(v) = params["email"].as_str().filter(|s| !s.is_empty()) {
            account.email = v.to_string();
        }
        if let Some(v) = params["avatar_url"].as_str().filter(|s| !s.is_empty()) {
            account.avatar_url = v.to_string();
        }

        account.login_state = LoginState::Available;
        account.login_state_reason = "凭据已保存".to_string();
        account.login_state_updated_at = chrono::Utc::now();
        account.last_check_at = Some(chrono::Utc::now());

        storage.update_account(&account)?;
        drop(storage);

        info!(account_id, "账号凭据已保存");

        Ok(StepResult {
            success: true,
            data: json!({ "account_id": account_id, "credentials_saved": true }),
            error: None,
        })
    }
}

#[async_trait]
impl AtomExecutor for AccountEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "account"
    }

    async fn execute(
        &self,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        match action {
            "account.set_state" => self.set_state(params).await,
            "account.save_credentials" => self.save_credentials(params).await,
            _ => Err(AppError::PlatformError(format!(
                "未知的 account 操作: {action}，可用: account.set_state, account.save_credentials"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_prefix() {
        let storage = Arc::new(Mutex::new(
            Storage::new(&std::env::temp_dir().join("test_account_engine.db")).unwrap(),
        ));
        let engine = AccountEngine::new(storage);
        assert!(engine.handles("account"));
        assert!(!engine.handles("email"));
    }
}
