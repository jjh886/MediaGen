use chrono::{Local, Utc};
use mediagen_browser_gateway::{
    BrowserSession, BrowserSessionPurpose, BROWSER_OS_METADATA_KEY, BROWSER_VERSION_METADATA_KEY,
};
use mediagen_core::PluginManifest;
use uuid::Uuid;

use crate::account_login::{default_tier, AccountLoginProfile};
use crate::net_client::proxy_url_with_auth;
use crate::store::{AccountRecord, AppStore};
use crate::store_helpers::state_releases_after_until;
use crate::store_helpers::{
    apply_account_profile, first_non_empty, proxy_url_from_session_metadata, reset_daily_usage,
    text_from_session_metadata,
};

impl AppStore {
    pub fn mark_login_check_result(
        &self,
        id: &str,
        login_state: &str,
        profile: Option<AccountLoginProfile>,
    ) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        crate::store_quota::remember(account);
        if login_state == "available" {
            if let Some(kind) = profile.as_ref().and_then(|p| p.quota_task_kind.as_deref()) {
                crate::store_quota::clear_confirmed(account, kind);
            }
        }
        let keep_quota = login_state == "available"
            && matches!(
                account.login_state.as_str(),
                "daily_quota_exhausted" | "total_quota_exhausted"
            )
            && profile
                .as_ref()
                .and_then(|profile| profile.quota_task_kind.as_deref())
                .is_some_and(|scope| {
                    account
                        .policy
                        .quota_task_kind
                        .as_deref()
                        .is_some_and(|old| (scope == "text") != (old == "text"))
                });
        let login_state = if keep_quota {
            account.login_state.clone()
        } else {
            login_state.into()
        };
        let login_state = login_state.as_str();
        account.policy.login_state_reason = login_state.to_string();
        account.policy.last_check_at = Some(Utc::now().to_rfc3339());
        if login_state == "available" {
            account.policy.quota_task_kind = None;
        }
        account.login_state = login_state.to_string();
        if !state_releases_after_until(login_state) {
            account.cooldown_until = None;
        }
        if let Some(profile) = profile {
            apply_account_profile(account, profile);
        }
        let account = account.clone();
        drop(accounts);
        self.save();
        Some(account)
    }

    pub(crate) fn select_account(
        &self,
        platform: &str,
        api_key: Option<&str>,
        excluded_ids: &[String],
        kind: &str,
    ) -> Option<AccountRecord> {
        self.select_account_for_task(platform, api_key, excluded_ids, kind, None)
    }

    pub(crate) fn select_account_for_task(
        &self,
        platform: &str,
        api_key: Option<&str>,
        excluded_ids: &[String],
        kind: &str,
        task_id: Option<&str>,
    ) -> Option<AccountRecord> {
        self.rollover_daily_usage();
        self.release_expired_cooldowns();
        let accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        let tasks = self.dispatch_tasks(task_id);
        let global = api_key.is_none_or(|key| self.global_api_key_allowed(key));
        accounts
            .iter()
            .filter(|account| {
                account.platform == platform
                    && !excluded_ids.contains(&account.id)
                    && crate::store_helpers::account_ready_for_kind(account, &tasks, kind)
                    && (global
                        || api_key.is_some_and(|key| {
                            crate::access_control::account_key_allowed(&account.api_key, key)
                        }))
            })
            .min_by_key(|account| (account.policy.priority, account.policy.last_used_at.clone()))
            .cloned()
    }
    /// 活跃执行器即使已被标记取消，退出前仍占用账号；恢复等待排除本任务旧记录。
    pub(crate) fn dispatch_tasks(&self, task_id: Option<&str>) -> Vec<crate::store::TaskRecord> {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let leases = self
            .protocol_leases
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        for task in &mut tasks {
            if leases.contains(&task.id) {
                task.status = "processing".into();
            }
        }
        tasks.retain(|task| task_id != Some(task.id.as_str()) || leases.contains(&task.id));
        drop(leases);
        tasks
    }

    pub(crate) fn mark_account_dispatched(&self, id: &str) {
        if let Some(account) = self
            .accounts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
            .find(|a| a.id == id)
        {
            account.policy.last_used_at = Some(Utc::now().to_rfc3339());
        }
    }
    pub fn mark_account_usage(&self, account_id: &str) {
        self.mark_account_usage_for(account_id, "image");
    }
    pub(crate) fn mark_account_usage_for(&self, account_id: &str, kind: &str) {
        let today = Local::now().date_naive().to_string();
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(account) = accounts.iter_mut().find(|account| account.id == account_id) {
            reset_daily_usage(account, &today);
            if kind == "text" {
                account.policy.text_used_today = account.policy.text_used_today.saturating_add(1);
                account.policy.text_used_total = account.policy.text_used_total.saturating_add(1);
            } else {
                account.used_today = account.used_today.saturating_add(1);
                account.used_total = account.used_total.saturating_add(1);
            }
            account.policy.last_success_at = Some(Utc::now().to_rfc3339());
        }
        drop(accounts);
        self.save();
    }
    pub(crate) fn record_account_failure(&self, id: &str, reason: &str, kind: &str) {
        if let Some(account) = self
            .accounts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
            .find(|account| account.id == id)
        {
            account.policy.login_state_reason = reason.into();
            if matches!(
                account.login_state.as_str(),
                "daily_quota_exhausted" | "total_quota_exhausted"
            ) {
                account.policy.quota_task_kind = Some(kind.into());
                crate::store_quota::remember(account);
            }
        }
        self.save();
    }
    pub fn upsert_from_browser_session(&self, session: &BrowserSession) -> Option<AccountRecord> {
        match session.purpose {
            BrowserSessionPurpose::AddAccount => {
                Some(self.create_from_browser_session(session, None, None))
            }
            BrowserSessionPurpose::ReloginAccount => session
                .account_id
                .as_deref()
                .and_then(|id| self.mark_available(id, None, session)),
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
            BrowserSessionPurpose::ReloginAccount => session
                .account_id
                .as_deref()
                .and_then(|id| self.mark_available(id, Some(profile), session)),
            _ => None,
        }
    }

    pub fn account_with_required_runtime_proxy(
        &self,
        mut account: AccountRecord,
    ) -> Result<AccountRecord, String> {
        let proxy_id = account.proxy_id.trim();
        if proxy_id.is_empty() {
            return Ok(account);
        }
        let proxy = self
            .proxy_by_id(proxy_id)
            .ok_or_else(|| format!("账号代理不存在或已删除: {proxy_id}"))?;
        if !proxy.enabled {
            return Err(format!("账号代理已停用: {proxy_id}"));
        }
        account.proxy_url = proxy_url_with_auth(&proxy.server, &proxy.username, &proxy.password);
        Ok(account)
    }

    fn create_from_browser_session(
        &self,
        session: &BrowserSession,
        manifest: Option<&PluginManifest>,
        profile: Option<AccountLoginProfile>,
    ) -> AccountRecord {
        let profile = profile.unwrap_or_default();
        // 重复登录更新同一上游身份，保留用户配置和用量，不建立第二个额度池。
        let _dispatch = self
            .account_dispatch
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !profile.user_id.is_empty() {
            let existing = self
                .list_accounts(Some(&session.platform))
                .into_iter()
                .find(|account| same_upstream_identity(account, &profile));
            if let Some(existing) = existing {
                return self
                    .mark_available(&existing.id, Some(profile), session)
                    .unwrap_or(existing);
            }
        }
        let tier = account_tier(&profile, manifest);
        let limits = manifest.and_then(|manifest| manifest.tiers.get(&tier));
        let account = AccountRecord {
            policy: crate::store_models::AccountPolicy {
                user_id: profile.user_id.clone(),
                text_concurrency_limit: limits
                    .map(|tier| tier.text_concurrency_limit)
                    .filter(|limit| *limit > 0)
                    .unwrap_or(crate::store_models::DEFAULT_TEXT_CONCURRENCY_LIMIT),
                text_daily_limit: limits.map(|tier| tier.text_daily_limit).unwrap_or(0),
                text_total_limit: limits.map(|tier| tier.text_total_limit).unwrap_or(0),
                ..Default::default()
            },
            id: format!("acc-{}", Uuid::new_v4()),
            platform: session.platform.clone(),
            name: account_name(&profile, &session.platform),
            email: first_non_empty(&[profile.email.as_str(), profile.user_id.as_str()]),
            avatar_url: profile.avatar_url,
            tier,
            login_state: "available".to_string(),
            cooldown_until: None,
            used_today: 0,
            used_today_date: Local::now().date_naive().to_string(),
            daily_limit: limits
                .map(|tier| tier.daily_limit.max(0) as u32)
                .unwrap_or(0),
            used_total: 0,
            total_limit: limits
                .map(|tier| tier.total_limit.max(0) as u32)
                .unwrap_or(0),
            concurrency_limit: limits
                .map(|tier| tier.concurrency_limit.max(1) as u32)
                .unwrap_or(1),
            task_interval_seconds: limits
                .map(|tier| tier.task_interval_seconds.max(0) as u32)
                .unwrap_or(0),
            proxy_url: proxy_url_from_session_metadata(&session.metadata).unwrap_or_default(),
            proxy_id: session.proxy_id.clone().unwrap_or_default(),
            browser_os: text_from_session_metadata(&session.metadata, BROWSER_OS_METADATA_KEY)
                .unwrap_or_default(),
            browser_version: text_from_session_metadata(
                &session.metadata,
                BROWSER_VERSION_METADATA_KEY,
            )
            .unwrap_or_default(),
            api_key: String::new(),
            profile_dir: session.profile_dir.clone(),
            browser_profile_dir: session.browser_profile_dir.clone(),
            created_at: Utc::now().to_rfc3339(),
        };
        self.insert_account(account.clone());
        account
    }

    fn mark_available(
        &self,
        id: &str,
        profile: Option<AccountLoginProfile>,
        session: &BrowserSession,
    ) -> Option<AccountRecord> {
        let mut accounts = self.accounts.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(profile) = &profile {
            if !profile.user_id.is_empty()
                && accounts.iter().any(|other| {
                    other.id != id
                        && other.platform == session.platform
                        && same_upstream_identity(other, profile)
                })
            {
                return None;
            }
        }
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        account.profile_dir = session.profile_dir.clone();
        account.browser_profile_dir = session.browser_profile_dir.clone();
        account.policy.last_check_at = Some(Utc::now().to_rfc3339());
        account.policy.login_state_reason.clear();
        account.login_state = "available".to_string();
        account.cooldown_until = None;
        apply_session_metadata(account, session);
        if let Some(profile) = profile {
            apply_account_profile(account, profile);
        }
        let account = account.clone();
        let browser_dir = account.effective_browser_profile_dir().to_string();
        if !browser_dir.trim().is_empty() {
            for other in accounts.iter_mut().filter(|other| {
                other.id != id && other.effective_browser_profile_dir() == browser_dir
            }) {
                other.proxy_url = account.proxy_url.clone();
                other.proxy_id = account.proxy_id.clone();
                other.browser_os = account.browser_os.clone();
                other.browser_version = account.browser_version.clone();
            }
        }
        drop(accounts);
        self.save();
        Some(account)
    }
}

fn account_name(profile: &AccountLoginProfile, platform: &str) -> String {
    let fallback = format!("{platform} 账号");
    first_non_empty(&[
        profile.username.as_str(),
        profile.email.as_str(),
        fallback.as_str(),
    ])
}

fn account_tier(profile: &AccountLoginProfile, manifest: Option<&PluginManifest>) -> String {
    let manifest_tier = manifest.map(default_tier).unwrap_or_default();
    first_non_empty(&[profile.tier.as_str(), manifest_tier.as_str()])
}

fn apply_session_metadata(account: &mut AccountRecord, session: &BrowserSession) {
    account.proxy_url = proxy_url_from_session_metadata(&session.metadata).unwrap_or_default();
    account.proxy_id = session.proxy_id.clone().unwrap_or_default();
    if let Some(value) = text_from_session_metadata(&session.metadata, BROWSER_OS_METADATA_KEY) {
        account.browser_os = value;
    }
    if let Some(value) = text_from_session_metadata(&session.metadata, BROWSER_VERSION_METADATA_KEY)
    {
        account.browser_version = value;
    }
}

fn same_upstream_identity(account: &AccountRecord, profile: &AccountLoginProfile) -> bool {
    if !account.policy.user_id.is_empty() {
        return account.policy.user_id == profile.user_id;
    }
    // 旧账号记录没有上游 ID，通过已保存的邮箱或旧 ID 回填身份，保留额度和用户配置。
    (!profile.email.trim().is_empty() && account.email.eq_ignore_ascii_case(profile.email.trim()))
        || (!profile.user_id.is_empty() && account.email == profile.user_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_identity_is_matched_without_merging_distinct_upstream_ids() {
        let mut account = crate::test_fixtures::account_record("legacy", "test", "Legacy", "");
        account.email = "user@example.test".into();
        let profile = AccountLoginProfile {
            user_id: "upstream-a".into(),
            email: "USER@example.test".into(),
            ..Default::default()
        };
        assert!(same_upstream_identity(&account, &profile));
        account.policy.user_id = "upstream-b".into();
        assert!(!same_upstream_identity(&account, &profile));
        account.policy.user_id = "upstream-a".into();
        assert!(same_upstream_identity(&account, &profile));
    }
}
