use crate::settings::AppSettings;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    #[serde(flatten)]
    pub policy: AccountPolicy,
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
    /// `used_today` 对应的服务端本地日期；日期变化时会自动清零每日用量。
    #[serde(default)]
    pub used_today_date: String,
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
    #[serde(default)]
    pub browser_profile_dir: String,
    pub created_at: String,
}

impl AccountRecord {
    pub fn effective_browser_profile_dir(&self) -> &str {
        if self.browser_profile_dir.trim().is_empty() {
            &self.profile_dir
        } else {
            &self.browser_profile_dir
        }
    }
}

pub(crate) const DEFAULT_TEXT_CONCURRENCY_LIMIT: u32 = 3;

/// 账号调度和健康状态；旧数据通过默认值兼容。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountPolicy {
    pub quota_holds: std::collections::BTreeMap<String, AccountQuotaHold>,
    pub quota_windows: Vec<serde_json::Value>,
    pub user_id: String,
    pub priority: i32,
    pub text_concurrency_limit: u32,
    pub text_daily_limit: u32,
    pub text_total_limit: u32,
    pub text_used_today: u32,
    pub text_used_total: u32,
    pub quota_task_kind: Option<String>,
    pub login_state_reason: String,
    pub last_check_at: Option<String>,
    pub last_used_at: Option<String>,
    pub last_success_at: Option<String>,
}
impl Default for AccountPolicy {
    fn default() -> Self {
        Self {
            quota_holds: Default::default(),
            quota_windows: Vec::new(),
            user_id: String::new(),
            priority: 0,
            text_concurrency_limit: DEFAULT_TEXT_CONCURRENCY_LIMIT,
            text_daily_limit: 0,
            text_total_limit: 0,
            text_used_today: 0,
            text_used_total: 0,
            quota_task_kind: None,
            login_state_reason: String::new(),
            last_check_at: None,
            last_used_at: None,
            last_success_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountQuotaHold {
    pub state: String,
    pub until: Option<String>,
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

/// 持久化生成任务的调度状态，与 aiduanju 的 generation_jobs 状态保持一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationJobState {
    Queued,
    RetryWait,
    Leased,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationJobRecord {
    pub task_id: String,
    pub owner_key: String,
    pub task_kind: String,
    pub request_json: String,
    pub credential_ciphertext: String,
    pub state: GenerationJobState,
    pub priority: i32,
    pub queue_seq: i64,
    pub next_attempt_at: String,
    pub retry_count: i32,
    pub last_failure_category: Option<String>,
    pub last_error_summary: Option<String>,
    pub lease_until: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeixinBindingRecord {
    pub id: String,
    pub status: String,
    pub ilink_bot_id: String,
    pub ilink_user_id: String,
    pub base_url: String,
    pub bot_token_ciphertext: String,
    pub context_token_ciphertext: String,
    pub get_updates_buf: String,
    pub activation_code_hash: String,
    pub activation_expires_at: Option<String>,
    pub last_inbound_at: Option<String>,
    pub last_outbound_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeixinPreferencesRecord {
    pub binding_id: String,
    pub generation_enabled: bool,
    pub direct_prompt_enabled: bool,
    pub default_model_id: String,
    pub default_aspect_ratio: String,
    pub default_max_images: u32,
    pub proactive_notifications_enabled: bool,
    pub notify_weixin_tasks: bool,
    pub notify_desktop_tasks: bool,
    pub notify_task_successes: bool,
    pub notify_no_accounts: bool,
    pub notify_account_actions: bool,
    pub quiet_hours_json: String,
    pub updated_at: String,
}

impl Default for WeixinPreferencesRecord {
    fn default() -> Self {
        Self {
            binding_id: "default".to_string(),
            generation_enabled: false,
            direct_prompt_enabled: false,
            default_model_id: String::new(),
            default_aspect_ratio: "9:16".to_string(),
            default_max_images: 1,
            proactive_notifications_enabled: false,
            notify_weixin_tasks: true,
            notify_desktop_tasks: true,
            notify_task_successes: false,
            notify_no_accounts: true,
            notify_account_actions: true,
            quiet_hours_json: String::new(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeixinMessageRecord {
    pub id: String,
    pub binding_id: String,
    pub direction: String,
    pub remote_message_id: Option<String>,
    pub client_id: String,
    pub message_type: String,
    pub content_json: String,
    pub status: String,
    pub attempt_count: i64,
    pub next_attempt_at: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub processed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeixinTaskLinkRecord {
    pub binding_id: String,
    pub inbound_message_id: String,
    pub task_id: String,
    pub result_delivery_status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeixinNotificationCursorRecord {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeixinAccountNotificationStateRecord {
    pub account_id: String,
    pub last_observed_state: String,
    pub alert_sent: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountUpdate {
    pub priority: Option<i32>,
    pub text_concurrency_limit: Option<u32>,
    pub text_daily_limit: Option<u32>,
    pub text_total_limit: Option<u32>,
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
    #[serde(default)]
    pub generation_jobs: Vec<GenerationJobRecord>,
    #[serde(default)]
    pub weixin_binding: Option<WeixinBindingRecord>,
    #[serde(default)]
    pub weixin_preferences: WeixinPreferencesRecord,
    #[serde(default)]
    pub weixin_messages: Vec<WeixinMessageRecord>,
    #[serde(default)]
    pub weixin_task_links: Vec<WeixinTaskLinkRecord>,
    #[serde(default)]
    pub weixin_notification_cursors: Vec<WeixinNotificationCursorRecord>,
    #[serde(default)]
    pub weixin_account_notification_states: Vec<WeixinAccountNotificationStateRecord>,
}
