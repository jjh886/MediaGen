use std::path::PathBuf;
use std::sync::Mutex;

use crate::settings::apply_settings_update;
use crate::store_entities::migrate_legacy_api_keys;
use crate::store_helpers::{
    apply_account_update, first_result_url, recover_interrupted_direct_tasks, reset_daily_usage,
    state_releases_after_until,
};
pub use crate::store_models::{
    AccountRecord, AccountUpdate, ApiKeyRecord, AuditLogRecord, GenerationJobRecord,
    GenerationJobState, ProxyRecord, StoreSnapshot, TaskEventRecord, TaskRecord,
    WeixinAccountNotificationStateRecord, WeixinBindingRecord, WeixinMessageRecord,
    WeixinNotificationCursorRecord, WeixinPreferencesRecord, WeixinTaskLinkRecord,
};
use crate::store_persistence::{read_store_snapshot, write_store_snapshot};
use chrono::{DateTime, Local, Utc};
use uuid::Uuid;

pub use crate::settings::{AppSettings, SettingsUpdate};

pub struct AppStore {
    pub(crate) session_affinity: Mutex<std::collections::HashMap<String, (String, i64)>>,
    pub(crate) path: PathBuf,
    pub(crate) account_dispatch: Mutex<()>,
    pub(crate) protocol_leases: Mutex<std::collections::HashSet<String>>,
    pub(crate) accounts: Mutex<Vec<AccountRecord>>,
    pub(crate) tasks: Mutex<Vec<TaskRecord>>,
    pub(crate) settings: Mutex<AppSettings>,
    pub(crate) api_keys: Mutex<Vec<ApiKeyRecord>>,
    pub(crate) proxies: Mutex<Vec<ProxyRecord>>,
    pub(crate) audit_logs: Mutex<Vec<AuditLogRecord>>,
    pub(crate) task_events: Mutex<Vec<TaskEventRecord>>,
    pub(crate) generation_jobs: Mutex<Vec<GenerationJobRecord>>,
    pub(crate) weixin_binding: Mutex<Option<WeixinBindingRecord>>,
    pub(crate) weixin_preferences: Mutex<WeixinPreferencesRecord>,
    pub(crate) weixin_messages: Mutex<Vec<WeixinMessageRecord>>,
    pub(crate) weixin_task_links: Mutex<Vec<WeixinTaskLinkRecord>>,
    pub(crate) weixin_notification_cursors: Mutex<Vec<WeixinNotificationCursorRecord>>,
    pub(crate) weixin_account_notification_states: Mutex<Vec<WeixinAccountNotificationStateRecord>>,
}

impl AppStore {
    pub fn load(path: PathBuf) -> Self {
        let mut snapshot = read_store_snapshot(&path);
        migrate_legacy_api_keys(&mut snapshot);
        for account in &mut snapshot.accounts {
            // 旧版只有媒体额度；迁移后不能把旧图片限额套用到新增文本通道。
            if account.policy.quota_task_kind.is_none()
                && matches!(
                    account.login_state.as_str(),
                    "daily_quota_exhausted" | "total_quota_exhausted"
                )
            {
                account.policy.quota_task_kind = Some("media".into());
            }
        }
        crate::image_api_response::sanitize_persisted_image_tasks(&mut snapshot.tasks);
        let recovered = recover_interrupted_direct_tasks(
            &mut snapshot.tasks,
            &snapshot.generation_jobs,
            &Utc::now().to_rfc3339(),
        );
        if recovered > 0 {
            eprintln!("[mediagen-server] 已结束 {recovered} 个上次退出时中断的同步生成任务");
        }
        let store = Self {
            session_affinity: Default::default(),
            path,
            account_dispatch: Mutex::new(()),
            protocol_leases: Default::default(),
            accounts: Mutex::new(snapshot.accounts),
            tasks: Mutex::new(snapshot.tasks),
            settings: Mutex::new(snapshot.settings),
            api_keys: Mutex::new(snapshot.api_keys),
            proxies: Mutex::new(snapshot.proxies),
            audit_logs: Mutex::new(snapshot.audit_logs),
            task_events: Mutex::new(snapshot.task_events),
            generation_jobs: Mutex::new(snapshot.generation_jobs),
            weixin_binding: Mutex::new(snapshot.weixin_binding),
            weixin_preferences: Mutex::new(snapshot.weixin_preferences),
            weixin_messages: Mutex::new(snapshot.weixin_messages),
            weixin_task_links: Mutex::new(snapshot.weixin_task_links),
            weixin_notification_cursors: Mutex::new(snapshot.weixin_notification_cursors),
            weixin_account_notification_states: Mutex::new(
                snapshot.weixin_account_notification_states,
            ),
        };
        store.save();
        store
    }

    pub fn list_accounts(&self, platform: Option<&str>) -> Vec<AccountRecord> {
        self.rollover_daily_usage();
        let accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        accounts
            .iter()
            .filter(|account| {
                platform.is_none_or(|value| value.is_empty() || account.platform == value)
            })
            .cloned()
            .collect()
    }

    pub fn account_by_id(&self, id: &str) -> Option<AccountRecord> {
        self.rollover_daily_usage();
        self.accounts
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .find(|account| account.id == id)
            .cloned()
    }

    pub fn first_available_account_for_key(
        &self,
        platform: &str,
        api_key: Option<&str>,
    ) -> Option<AccountRecord> {
        self.first_available_account_for_key_excluding(platform, api_key, &[])
    }

    pub fn first_available_account_for_key_excluding(
        &self,
        platform: &str,
        api_key: Option<&str>,
        excluded_ids: &[String],
    ) -> Option<AccountRecord> {
        self.select_account(platform, api_key, excluded_ids, "image")
    }

    /// 把“选择账号”和“记录账号占用”包在同一个临界区，避免并发请求超发同一账号。
    pub(crate) fn with_account_dispatch<T>(&self, action: impl FnOnce() -> T) -> T {
        let _guard = self
            .account_dispatch
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        action()
    }

    pub fn settings(&self) -> AppSettings {
        self.settings
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn update_settings(&self, update: SettingsUpdate) -> AppSettings {
        self.replace_api_keys_from_settings_update(update.api_keys.as_deref());
        let mut settings = self.settings.lock().unwrap_or_else(|err| err.into_inner());
        apply_settings_update(&mut settings, update);
        settings.api_keys.clear();
        let updated = settings.clone();
        drop(settings);
        self.save();
        updated
    }

    pub fn list_tasks(&self, platform: Option<&str>) -> Vec<TaskRecord> {
        let mut tasks: Vec<_> = self
            .tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .filter(|task| platform.is_none_or(|value| value.is_empty() || task.platform == value))
            .cloned()
            .collect();
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        tasks
    }

    pub fn task_by_id(&self, id: &str) -> Option<TaskRecord> {
        self.tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .find(|task| task.id == id)
            .cloned()
    }

    pub fn insert_task(&self, task: TaskRecord) {
        if task.status == "processing" {
            if let Some(id) = &task.account_id {
                self.mark_account_dispatched(id);
            }
        }
        self.tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(task);
        self.save();
    }

    pub fn add_task_event(&self, task_id: &str, level: &str, message: String) {
        self.task_events
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(TaskEventRecord {
                id: format!("event-{}", Uuid::new_v4()),
                task_id: task_id.to_string(),
                level: level.to_string(),
                message,
                created_at: Utc::now().to_rfc3339(),
            });
        self.save();
    }

    pub fn complete_task(&self, id: &str, result: serde_json::Value) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            if task.status == "cancelled" {
                return;
            }
            task.status = "completed".to_string();
            task.result_data = Some(result.to_string());
            task.completed_at = Some(Utc::now().to_rfc3339());
            if let Some(url) = first_result_url(&result) {
                task.result_url = Some(url);
            }
        })
    }

    pub(crate) fn clear_task_progress(&self, id: &str) {
        self.update_task(id, |task| task.result_data = None);
    }

    pub(crate) fn save_task_progress(
        &self,
        id: &str,
        progress: serde_json::Value,
    ) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            if task.status == "processing" {
                task.result_data = Some(progress.to_string());
            }
        })
    }

    pub fn set_task_account(&self, id: &str, account_id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.account_id = Some(account_id.to_string());
        })
    }

    pub fn start_task(&self, id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "processing".to_string();
        })
    }

    pub fn fail_task(&self, id: &str, message: String) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            if task.status == "cancelled" {
                return;
            }
            task.status = "failed".to_string();
            task.error_message = Some(message);
            task.completed_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub fn require_task_verification(&self, id: &str, message: String) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "verification_required".to_string();
            task.error_message = Some(message);
        })
    }

    pub fn resume_task_after_verification(&self, id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "queued".to_string();
            task.error_message = None;
            task.completed_at = None;
        })
    }

    pub fn cancel_task(&self, id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "cancelled".to_string();
            task.error_message = Some("用户取消".to_string());
            task.completed_at = Some(Utc::now().to_rfc3339());
        })
    }

    fn update_task(&self, id: &str, update: impl FnOnce(&mut TaskRecord)) -> Option<TaskRecord> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let task = tasks.iter_mut().find(|task| task.id == id)?;
        update(task);
        let task = task.clone();
        drop(tasks);
        self.save();
        Some(task)
    }

    pub fn update_account(&self, id: &str, update: AccountUpdate) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        apply_account_update(account, update);
        let account = account.clone();
        drop(accounts);
        self.save();
        Some(account)
    }

    pub fn delete_account(&self, id: &str) -> bool {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let before = accounts.len();
        accounts.retain(|account| account.id != id);
        let changed = accounts.len() != before;
        drop(accounts);
        if changed {
            self.save();
        }
        changed
    }

    pub fn toggle_account(&self, id: &str, disabled: bool) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        account.login_state = if disabled { "disabled" } else { "available" }.to_string();
        let account = account.clone();
        drop(accounts);
        self.save();
        Some(account)
    }

    pub fn mark_account_state(&self, id: &str, login_state: &str) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        crate::store_quota::remember(account);
        account.policy.login_state_reason = login_state.to_string();
        account.policy.last_check_at = Some(Utc::now().to_rfc3339());
        if login_state == "available" {
            account.policy.quota_task_kind = None;
        }
        account.login_state = login_state.to_string();
        if login_state != "cooldown" {
            account.cooldown_until = None;
        }
        let account = account.clone();
        drop(accounts);
        self.save();
        Some(account)
    }

    pub fn mark_account_state_for_seconds(
        &self,
        id: &str,
        login_state: &str,
        seconds: i64,
    ) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        crate::store_quota::remember(account);
        account.policy.login_state_reason = login_state.to_string();
        account.policy.last_check_at = Some(Utc::now().to_rfc3339());
        if login_state == "available" {
            account.policy.quota_task_kind = None;
        }
        account.login_state = login_state.to_string();
        account.cooldown_until = (state_releases_after_until(login_state) && seconds > 0)
            .then(|| (Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339());
        let account = account.clone();
        drop(accounts);
        self.save();
        Some(account)
    }

    pub fn insert_account(&self, account: AccountRecord) {
        self.accounts
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(account);
        self.save();
    }

    pub(crate) fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let settings = self.settings.lock().unwrap_or_else(|err| err.into_inner());
        let api_keys = self.api_keys.lock().unwrap_or_else(|err| err.into_inner());
        let proxies = self.proxies.lock().unwrap_or_else(|err| err.into_inner());
        let audit_logs = self
            .audit_logs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let task_events = self
            .task_events
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let generation_jobs = self
            .generation_jobs
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let weixin_binding = self
            .weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let weixin_preferences = self
            .weixin_preferences
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let weixin_messages = self
            .weixin_messages
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let weixin_task_links = self
            .weixin_task_links
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let weixin_notification_cursors = self
            .weixin_notification_cursors
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let weixin_account_notification_states = self
            .weixin_account_notification_states
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let snapshot = StoreSnapshot {
            accounts: accounts.clone(),
            tasks: tasks.clone(),
            settings: settings.clone(),
            api_keys: api_keys.clone(),
            proxies: proxies.clone(),
            audit_logs: audit_logs.clone(),
            task_events: task_events.clone(),
            generation_jobs: generation_jobs.clone(),
            weixin_binding: weixin_binding.clone(),
            weixin_preferences: weixin_preferences.clone(),
            weixin_messages: weixin_messages.clone(),
            weixin_task_links: weixin_task_links.clone(),
            weixin_notification_cursors: weixin_notification_cursors.clone(),
            weixin_account_notification_states: weixin_account_notification_states.clone(),
        };
        let _ = write_store_snapshot(&self.path, &snapshot);
    }

    pub(crate) fn release_expired_cooldowns(&self) {
        let now = Utc::now();
        let mut changed = false;
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        for account in accounts.iter_mut() {
            if account_hold_has_expired(account, now) {
                if let Some(kind) = account.policy.quota_task_kind.clone() {
                    crate::store_quota::clear_confirmed(account, &kind);
                }
                account.login_state = "available".to_string();
                account.cooldown_until = None;
                account.policy.quota_task_kind = None;
                account.policy.login_state_reason.clear();
                changed = true;
            }
        }
        drop(accounts);
        if changed {
            self.save();
        }
    }

    pub(crate) fn rollover_daily_usage(&self) {
        let today = current_usage_date();
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let mut changed = false;
        for account in accounts.iter_mut() {
            changed |= reset_daily_usage(account, &today);
        }
        drop(accounts);
        if changed {
            self.save();
        }
    }
}

fn current_usage_date() -> String {
    // “今日”跟随运行 MediaGen 的机器自然日，符合本机版和服务器版的直觉。
    Local::now().date_naive().to_string()
}

fn account_hold_has_expired(account: &AccountRecord, now: DateTime<Utc>) -> bool {
    state_releases_after_until(&account.login_state)
        && account
            .cooldown_until
            .as_deref()
            .and_then(parse_time)
            .is_some_and(|until| until <= now)
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}
