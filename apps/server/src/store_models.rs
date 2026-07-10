use crate::settings::AppSettings;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    pub id: String,
    pub platform: String,
    pub name: String,
    pub email: String,
    pub avatar_url: String,
    pub tier: String,
    pub login_state: String,
    #[serde(default)]
    pub cooldown_until: Option<String>,
    pub used_today: u32,
    pub daily_limit: u32,
    pub used_total: u32,
    pub total_limit: u32,
    pub concurrency_limit: u32,
    pub task_interval_seconds: u32,
    pub proxy_url: String,
    #[serde(default)]
    pub proxy_id: String,
    #[serde(default)]
    pub browser_os: String,
    #[serde(default)]
    pub browser_version: String,
    pub api_key: String,
    pub profile_dir: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub platform: String,
    pub task_kind: String,
    pub model_id: String,
    pub prompt: String,
    pub account_id: Option<String>,
    pub status: String,
    pub result_url: Option<String>,
    pub storage_key: Option<String>,
    pub error_message: Option<String>,
    pub result_data: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountUpdate {
    pub tier: Option<String>,
    pub concurrency_limit: Option<u32>,
    pub daily_limit: Option<u32>,
    pub total_limit: Option<u32>,
    pub task_interval_seconds: Option<u32>,
    pub proxy_url: Option<String>,
    pub proxy_id: Option<String>,
    pub browser_os: Option<String>,
    pub browser_version: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub key_suffix: String,
    pub rpm_limit: u32,
    pub daily_limit: u32,
    pub expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRecord {
    pub id: String,
    pub name: String,
    pub server: String,
    pub username: String,
    pub password: String,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogRecord {
    pub id: String,
    pub timestamp: String,
    pub module: String,
    pub level: String,
    pub summary: String,
    pub detail: String,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEventRecord {
    pub id: String,
    pub task_id: String,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct StoreSnapshot {
    pub accounts: Vec<AccountRecord>,
    #[serde(default)]
    pub tasks: Vec<TaskRecord>,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub api_keys: Vec<ApiKeyRecord>,
    #[serde(default)]
    pub proxies: Vec<ProxyRecord>,
    #[serde(default)]
    pub audit_logs: Vec<AuditLogRecord>,
    #[serde(default)]
    pub task_events: Vec<TaskEventRecord>,
}
