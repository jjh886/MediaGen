use std::path::PathBuf;
use std::sync::Mutex;

use crate::account_login::AccountLoginProfile;
use crate::settings::apply_settings_update;
use crate::store_entities::migrate_legacy_api_keys;
use crate::store_helpers::{
    account_ready_for_dispatch, apply_account_profile, apply_account_update, first_result_url,
    recover_interrupted_direct_tasks,
};
pub use crate::store_models::{
    AccountRecord, AccountUpdate, ApiKeyRecord, AuditLogRecord, GenerationJobRecord,
    GenerationJobState, ProxyRecord, StoreSnapshot, TaskEventRecord, TaskRecord,
};
use crate::store_persistence::{read_store_snapshot, write_store_snapshot};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub use crate::settings::{AppSettings, SettingsUpdate};

pub struct AppStore {
    pub(crate) path: PathBuf,
    account_dispatch: Mutex<()>,
    pub(crate) accounts: Mutex<Vec<AccountRecord>>,
    pub(crate) tasks: Mutex<Vec<TaskRecord>>,
    pub(crate) settings: Mutex<AppSettings>,
    pub(crate) api_keys: Mutex<Vec<ApiKeyRecord>>,
    pub(crate) proxies: Mutex<Vec<ProxyRecord>>,
    pub(crate) audit_logs: Mutex<Vec<AuditLogRecord>>,
    pub(crate) task_events: Mutex<Vec<TaskEventRecord>>,
    pub(crate) generation_jobs: Mutex<Vec<GenerationJobRecord>>,
}

impl AppStore {
    pub fn load(path: PathBuf) -> Self {
        let mut snapshot = read_store_snapshot(&path);
        migrate_legacy_api_keys(&mut snapshot);
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
            path,
            account_dispatch: Mutex::new(()),
            accounts: Mutex::new(snapshot.accounts),
            tasks: Mutex::new(snapshot.tasks),
            settings: Mutex::new(snapshot.settings),
            api_keys: Mutex::new(snapshot.api_keys),
            proxies: Mutex::new(snapshot.proxies),
            audit_logs: Mutex::new(snapshot.audit_logs),
            task_events: Mutex::new(snapshot.task_events),
            generation_jobs: Mutex::new(snapshot.generation_jobs),
        };
        store.save();
        store
    }

    pub fn list_accounts(&self, platform: Option<&str>) -> Vec<AccountRecord> {
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
        self.release_expired_cooldowns();
        let accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let mut candidates = accounts.iter().filter(|account| {
            account.platform == platform
                && !excluded_ids.iter().any(|id| id == &account.id)
                && account_ready_for_dispatch(account, &tasks)
        });
        if let Some(key) = api_key.map(str::trim).filter(|key| !key.is_empty()) {
            if self.global_api_key_allowed(key) {
                candidates.next().cloned()
            } else {
                candidates
                    .find(|account| {
                        crate::access_control::account_key_allowed(&account.api_key, key)
                    })
                    .cloned()
            }
        } else {
            candidates.next().cloned()
        }
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

    pub fn mark_account_usage(&self, account_id: &str) {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(account) = accounts.iter_mut().find(|account| account.id == account_id) {
            account.used_today = account.used_today.saturating_add(1);
            account.used_total = account.used_total.saturating_add(1);
        }
        drop(accounts);
        self.save();
    }

    pub fn complete_task(&self, id: &str, result: serde_json::Value) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "completed".to_string();
            task.result_data = Some(result.to_string());
            task.completed_at = Some(Utc::now().to_rfc3339());
            if let Some(url) = first_result_url(&result) {
                task.result_url = Some(url);
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

    pub fn mark_login_check_result(
        &self,
        id: &str,
        login_state: &str,
        profile: Option<AccountLoginProfile>,
    ) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        account.login_state = login_state.to_string();
        if let Some(profile) = profile {
            apply_account_profile(account, profile);
        }
        let account = account.clone();
        drop(accounts);
        self.save();
        Some(account)
    }

    pub fn mark_account_state(&self, id: &str, login_state: &str) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
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
        account.login_state = login_state.to_string();
        account.cooldown_until = (login_state == "cooldown" && seconds > 0)
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
        let snapshot = StoreSnapshot {
            accounts: accounts.clone(),
            tasks: tasks.clone(),
            settings: settings.clone(),
            api_keys: api_keys.clone(),
            proxies: proxies.clone(),
            audit_logs: audit_logs.clone(),
            task_events: task_events.clone(),
            generation_jobs: generation_jobs.clone(),
        };
        let _ = write_store_snapshot(&self.path, &snapshot);
    }

    fn release_expired_cooldowns(&self) {
        let now = Utc::now();
        let mut changed = false;
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        for account in accounts.iter_mut() {
            if cooldown_has_expired(account, now) {
                account.login_state = "available".to_string();
                account.cooldown_until = None;
                changed = true;
            }
        }
        drop(accounts);
        if changed {
            self.save();
        }
    }
}

fn cooldown_has_expired(account: &AccountRecord, now: DateTime<Utc>) -> bool {
    account.login_state == "cooldown"
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
