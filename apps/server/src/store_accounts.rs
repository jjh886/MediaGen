use chrono::Utc;
use mediagen_browser_gateway::{
    BrowserSession, BrowserSessionPurpose, BROWSER_OS_METADATA_KEY, BROWSER_VERSION_METADATA_KEY,
};
use mediagen_core::PluginManifest;
use uuid::Uuid;

use crate::account_login::{default_tier, AccountLoginProfile};
use crate::net_client::proxy_url_with_auth;
use crate::store::{AccountRecord, AppStore};
use crate::store_helpers::{
    apply_account_profile, first_non_empty, proxy_url_from_session_metadata,
    text_from_session_metadata,
};

impl AppStore {
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

    pub fn account_with_runtime_proxy(&self, mut account: AccountRecord) -> AccountRecord {
        let proxy_id = account.proxy_id.trim();
        if proxy_id.is_empty() {
            return account;
        }
        account.proxy_url = self
            .proxy_by_id(proxy_id)
            .filter(|proxy| proxy.enabled)
            .map(|proxy| proxy_url_with_auth(&proxy.server, &proxy.username, &proxy.password))
            .unwrap_or_default();
        account
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
        let tier = account_tier(&profile, manifest);
        let account = AccountRecord {
            id: format!("acc-{}", Uuid::new_v4()),
            platform: session.platform.clone(),
            name: account_name(&profile, &session.platform),
            email: first_non_empty(&[profile.email.as_str(), profile.user_id.as_str()]),
            avatar_url: profile.avatar_url,
            tier,
            login_state: "available".to_string(),
            cooldown_until: None,
            used_today: 0,
            daily_limit: 100,
            used_total: 0,
            total_limit: 5000,
            concurrency_limit: 1,
            task_interval_seconds: 0,
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
        let account = accounts.iter_mut().find(|account| account.id == id)?;
        account.login_state = "available".to_string();
        account.cooldown_until = None;
        apply_session_metadata(account, session);
        if let Some(profile) = profile {
            apply_account_profile(account, profile);
        }
        let account = account.clone();
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
    if let Some(proxy_url) = proxy_url_from_session_metadata(&session.metadata) {
        account.proxy_url = proxy_url;
    }
    if let Some(proxy_id) = non_empty(session.proxy_id.as_deref()) {
        account.proxy_id = proxy_id.to_string();
    }
    if let Some(value) = text_from_session_metadata(&session.metadata, BROWSER_OS_METADATA_KEY) {
        account.browser_os = value;
    }
    if let Some(value) = text_from_session_metadata(&session.metadata, BROWSER_VERSION_METADATA_KEY)
    {
        account.browser_version = value;
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
