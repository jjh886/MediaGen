use std::path::PathBuf;
use std::sync::Mutex;

use crate::access_control::{account_key_allowed, groups_for_key, normalize_lines};
use crate::account_login::{default_tier, AccountLoginProfile};
use crate::settings::apply_settings_update;
use chrono::Utc;
use mediagen_browser_gateway::{BrowserSession, BrowserSessionPurpose};
use mediagen_core::PluginManifest;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::settings::{AppSettings, SettingsUpdate};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    pub id: String,
    pub platform: String,
    pub name: String,
    pub email: String,
    pub avatar_url: String,
    pub tier: String,
    pub login_state: String,
    pub used_today: u32,
    pub daily_limit: u32,
    pub used_total: u32,
    pub total_limit: u32,
    pub concurrency_limit: u32,
    pub task_interval_seconds: u32,
    pub group_name: String,
    pub proxy_url: String,
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
    pub group_name: Option<String>,
    pub proxy_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct StoreSnapshot {
    accounts: Vec<AccountRecord>,
    #[serde(default)]
    tasks: Vec<TaskRecord>,
    #[serde(default)]
    settings: AppSettings,
}

pub struct AppStore {
    path: PathBuf,
    accounts: Mutex<Vec<AccountRecord>>,
    tasks: Mutex<Vec<TaskRecord>>,
    settings: Mutex<AppSettings>,
}

impl AppStore {
    pub fn load(path: PathBuf) -> Self {
        let snapshot = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<StoreSnapshot>(&raw).ok())
            .unwrap_or_default();
        Self {
            path,
            accounts: Mutex::new(snapshot.accounts),
            tasks: Mutex::new(snapshot.tasks),
            settings: Mutex::new(snapshot.settings),
        }
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

    pub fn first_available_account_for_key(
        &self,
        platform: &str,
        api_key: Option<&str>,
    ) -> Option<AccountRecord> {
        let accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let mut candidates = accounts
            .iter()
            .filter(|account| account.platform == platform && account.login_state == "available");
        if let Some(key) = api_key.map(str::trim).filter(|key| !key.is_empty()) {
            let settings = self.settings.lock().unwrap_or_else(|err| err.into_inner());
            let groups = groups_for_key(&settings.api_keys, key);
            candidates
                .find(|account| account_allowed(account, key, &groups))
                .cloned()
        } else {
            candidates.next().cloned()
        }
    }

    pub fn settings(&self) -> AppSettings {
        self.settings
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn update_settings(&self, update: SettingsUpdate) -> AppSettings {
        let mut settings = self.settings.lock().unwrap_or_else(|err| err.into_inner());
        apply_settings_update(&mut settings, update);
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

    pub fn insert_task(&self, task: TaskRecord) {
        self.tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(task);
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

    pub fn fail_task(&self, id: &str, message: String) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "failed".to_string();
            task.error_message = Some(message);
            task.completed_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub fn cancel_task(&self, id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "failed".to_string();
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

    pub fn upsert_from_browser_session(&self, session: &BrowserSession) -> Option<AccountRecord> {
        match session.purpose {
            BrowserSessionPurpose::AddAccount => {
                Some(self.create_from_browser_session(session, None, None))
            }
            BrowserSessionPurpose::ReloginAccount => {
                if let Some(id) = &session.account_id {
                    self.mark_available(
                        id,
                        None,
                        proxy_url_from_session_metadata(&session.metadata),
                    )
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn upsert_from_browser_session_with_profile(
        &self,
        session: &BrowserSession,
        manifest: &PluginManifest,
        profile: AccountLoginProfile,
    ) -> Option<AccountRecord> {
        match session.purpose {
            BrowserSessionPurpose::AddAccount => {
                Some(self.create_from_browser_session(session, Some(manifest), Some(profile)))
            }
            BrowserSessionPurpose::ReloginAccount => session.account_id.as_deref().and_then(|id| {
                self.mark_available(
                    id,
                    Some(profile),
                    proxy_url_from_session_metadata(&session.metadata),
                )
            }),
            _ => None,
        }
    }

    pub fn update_account(&self, id: &str, update: AccountUpdate) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        if let Some(value) = update.tier {
            account.tier = value;
        }
        if let Some(value) = update.concurrency_limit {
            account.concurrency_limit = value;
        }
        if let Some(value) = update.daily_limit {
            account.daily_limit = value;
        }
        if let Some(value) = update.total_limit {
            account.total_limit = value;
        }
        if let Some(value) = update.task_interval_seconds {
            account.task_interval_seconds = value;
        }
        if let Some(value) = update.group_name {
            account.group_name = value;
        }
        if let Some(value) = update.proxy_url {
            account.proxy_url = value;
        }
        if let Some(value) = update.api_key {
            account.api_key = normalize_lines(&value);
        }
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
            if !profile.username.trim().is_empty() {
                account.name = profile.username;
            }
            if !profile.email.trim().is_empty() {
                account.email = profile.email;
            }
            if !profile.avatar_url.trim().is_empty() {
                account.avatar_url = profile.avatar_url;
            }
            if !profile.tier.trim().is_empty() {
                account.tier = profile.tier;
            }
        }
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

    fn create_from_browser_session(
        &self,
        session: &BrowserSession,
        manifest: Option<&PluginManifest>,
        profile: Option<AccountLoginProfile>,
    ) -> AccountRecord {
        let now = Utc::now().to_rfc3339();
        let profile = profile.unwrap_or_default();
        let fallback_name = format!("{} 账号", session.platform);
        let manifest_tier = manifest.map(default_tier).unwrap_or_default();
        let name = first_non_empty(&[
            profile.username.as_str(),
            profile.email.as_str(),
            fallback_name.as_str(),
        ]);
        let email = first_non_empty(&[profile.email.as_str(), profile.user_id.as_str()]);
        let tier = first_non_empty(&[profile.tier.as_str(), manifest_tier.as_str()]);
        let account = AccountRecord {
            id: format!("acc-{}", Uuid::new_v4()),
            platform: session.platform.clone(),
            name,
            email,
            avatar_url: profile.avatar_url,
            tier,
            login_state: "available".to_string(),
            used_today: 0,
            daily_limit: 100,
            used_total: 0,
            total_limit: 5000,
            concurrency_limit: 1,
            task_interval_seconds: 0,
            group_name: "默认分组".to_string(),
            proxy_url: proxy_url_from_session_metadata(&session.metadata).unwrap_or_default(),
            api_key: String::new(),
            profile_dir: session.profile_dir.clone(),
            created_at: now,
        };
        self.accounts
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(account.clone());
        self.save();
        account
    }

    fn mark_available(
        &self,
        id: &str,
        profile: Option<AccountLoginProfile>,
        proxy_url: Option<String>,
    ) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        account.login_state = "available".to_string();
        if let Some(proxy_url) = proxy_url {
            account.proxy_url = proxy_url;
        }
        if let Some(profile) = profile {
            if !profile.username.trim().is_empty() {
                account.name = profile.username;
            }
            if !profile.email.trim().is_empty() {
                account.email = profile.email;
            }
            if !profile.avatar_url.trim().is_empty() {
                account.avatar_url = profile.avatar_url;
            }
            if !profile.tier.trim().is_empty() {
                account.tier = profile.tier;
            }
        }
        let account = account.clone();
        drop(accounts);
        self.save();
        Some(account)
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let settings = self.settings.lock().unwrap_or_else(|err| err.into_inner());
        let snapshot = StoreSnapshot {
            accounts: accounts.clone(),
            tasks: tasks.clone(),
            settings: settings.clone(),
        };
        if let Ok(raw) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(&self.path, raw);
        }
    }
}

fn account_allowed(account: &AccountRecord, key: &str, groups: &[String]) -> bool {
    account_key_allowed(&account.api_key, key)
        || groups
            .iter()
            .any(|group| group.trim() == account.group_name.trim())
}

fn proxy_url_from_session_metadata(
    metadata: &std::collections::HashMap<String, serde_json::Value>,
) -> Option<String> {
    metadata
        .get("proxy_server")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn first_result_url(result: &serde_json::Value) -> Option<String> {
    result
        .get("images")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("url"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}
