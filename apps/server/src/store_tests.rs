use std::collections::HashMap;

use chrono::Utc;
use mediagen_browser_gateway::{
    BrowserSession, BrowserSessionPurpose, BrowserSessionStatus, BrowserViewport,
};
use mediagen_core::{PluginManifest, TierDef};
use serde_json::json;

use crate::account_login::AccountLoginProfile;
use crate::store::{AccountRecord, AccountUpdate, AppStore, ProxyRecord, SettingsUpdate};
use crate::test_fixtures::account_record;

#[path = "store_task_tests.rs"]
mod tasks;

#[test]
fn api_key_selects_accounts_by_global_or_explicit_key() {
    let store = temp_store("key-scope");
    store.update_settings(SettingsUpdate {
        api_keys: Some("global-key\nplain-key".to_string()),
        ..SettingsUpdate::default()
    });
    store.insert_account(account("acc-default", ""));
    store.insert_account(account("acc-explicit", "account-only-key"));

    assert_eq!(
        store
            .first_available_account_for_key("platform-a", Some("global-key"))
            .map(|item| item.id),
        Some("acc-default".to_string())
    );
    assert_eq!(
        store
            .first_available_account_for_key("platform-a", Some("plain-key"))
            .map(|item| item.id),
        Some("acc-default".to_string())
    );
    assert_eq!(
        store
            .first_available_account_for_key("platform-a", Some("account-only-key"))
            .map(|item| item.id),
        Some("acc-explicit".to_string())
    );
    assert!(store
        .first_available_account_for_key("platform-a", Some("unknown-key"))
        .is_none());
}

#[test]
fn settings_and_account_updates_are_normalized_and_persisted() {
    let store = temp_store("settings-persist");
    update_and_assert_settings(&store);
    store.insert_account(account("acc-1", "key-a,key-b"));
    let updated = update_account_config(&store);
    assert_updated_account(&updated);
    assert_reloaded_settings_and_account();
}

#[test]
fn shared_browser_accounts_keep_private_credentials_and_sync_runtime_settings() {
    let store = temp_store("shared-browser-settings");
    let mut source = account("source", "");
    source.profile_dir = "/profiles/source".into();
    source.browser_profile_dir = "/profiles/browser-shared".into();
    let mut target = account("target", "");
    target.profile_dir = "/profiles/target".into();
    target.browser_profile_dir = "/profiles/browser-shared".into();
    store.insert_account(source);
    store.insert_account(target);

    store.update_account(
        "source",
        AccountUpdate {
            proxy_id: Some("proxy-1".into()),
            browser_os: Some("linux".into()),
            browser_version: Some("146".into()),
            ..AccountUpdate::default()
        },
    );
    let target = store.account_by_id("target").unwrap();
    assert_eq!(target.profile_dir, "/profiles/target");
    assert_eq!(target.proxy_id, "proxy-1");
    assert_eq!(target.browser_os, "linux");
    assert_eq!(target.browser_version, "146");

    let mut relogin = session(BrowserSessionPurpose::ReloginAccount, Some("target".into()));
    relogin.profile_dir = "/profiles/target".into();
    relogin.browser_profile_dir = "/profiles/browser-shared".into();
    relogin.metadata.insert("browser_os".into(), json!("macos"));
    relogin
        .metadata
        .insert("browser_version".into(), json!("145"));
    store.upsert_from_browser_session(&relogin).unwrap();
    let source = store.account_by_id("source").unwrap();
    assert_eq!(source.browser_os, "macos");
    assert_eq!(source.browser_version, "145");
    assert_eq!(source.profile_dir, "/profiles/source");
}

fn update_and_assert_settings(store: &AppStore) {
    let settings = store.update_settings(SettingsUpdate {
        api_keys: Some(" key-a = legacy \n\n key-b ".to_string()),
        image_task_concurrency: Some(8),
        ..SettingsUpdate::default()
    });
    assert_eq!(settings.api_keys, "");
    assert!(store.api_key_allowed("key-a"));
    assert!(store.api_key_allowed("key-b"));
    assert_eq!(store.list_api_keys().len(), 2);
    assert_eq!(settings.image_task_concurrency, 8);
}

fn update_account_config(store: &AppStore) -> AccountRecord {
    store
        .update_account(
            "acc-1",
            AccountUpdate {
                priority: None,
                text_concurrency_limit: None,
                text_daily_limit: None,
                text_total_limit: None,
                tier: Some("pro".to_string()),
                concurrency_limit: Some(3),
                daily_limit: Some(20),
                total_limit: Some(200),
                task_interval_seconds: Some(9),
                proxy_url: Some("socks5://127.0.0.1:1080".to_string()),
                proxy_id: None,
                browser_os: Some("windows".to_string()),
                browser_version: Some("126".to_string()),
                api_key: Some(" key-c, key-d\nkey-e ".to_string()),
            },
        )
        .unwrap()
}

fn assert_updated_account(updated: &AccountRecord) {
    assert_eq!(updated.tier, "pro");
    assert_eq!(updated.concurrency_limit, 3);
    assert_eq!(updated.browser_os, "windows");
    assert_eq!(updated.browser_version, "126");
    assert!(updated.api_key.contains("sha256:"));
    assert!(crate::access_control::account_key_allowed(
        &updated.api_key,
        "key-d"
    ));
}

fn assert_reloaded_settings_and_account() {
    let reloaded = AppStore::load(store_path("settings-persist"));
    assert_eq!(reloaded.settings().api_keys, "");
    assert!(reloaded.api_key_allowed("key-a"));
    assert_eq!(reloaded.settings().image_task_concurrency, 8);
    assert_eq!(
        reloaded.list_accounts(None)[0].proxy_url,
        "socks5://127.0.0.1:1080"
    );
    assert_eq!(reloaded.list_accounts(None)[0].browser_version, "126");
}

#[test]
fn account_toggle_delete_and_login_check_update_state() {
    let store = temp_store("account-state");
    store.insert_account(account("acc-1", ""));

    assert_eq!(
        store.toggle_account("acc-1", true).unwrap().login_state,
        "disabled"
    );
    assert_eq!(
        store
            .mark_account_state("acc-1", "cooldown")
            .unwrap()
            .login_state,
        "cooldown"
    );
    assert_eq!(
        store
            .mark_login_check_result(
                "acc-1",
                "available",
                Some(AccountLoginProfile {
                    username: "Alice".to_string(),
                    email: "alice@example.test".to_string(),
                    avatar_url: "https://example.test/a.png".to_string(),
                    tier: "plus".to_string(),
                    ..AccountLoginProfile::default()
                }),
            )
            .unwrap()
            .email,
        "alice@example.test"
    );
    assert!(store.delete_account("acc-1"));
    assert!(!store.delete_account("missing"));
}

#[test]
fn account_list_rolls_legacy_daily_usage_into_current_date() {
    let store = temp_store("account-daily-rollover");
    let mut saved = account("acc-1", "");
    saved.used_today = 957;
    saved.used_today_date.clear();
    saved.used_total = 957;
    saved.login_state = "daily_quota_exhausted".to_string();
    store.insert_account(saved);

    let account = store.list_accounts(None).remove(0);

    assert_eq!(account.used_today, 0);
    assert_eq!(account.used_total, 957);
    assert!(!account.used_today_date.is_empty());
    assert_eq!(account.login_state, "available");
}

#[test]
fn management_entities_create_delete_and_persist() {
    let store = temp_store("management-entities");

    let proxy = store.create_proxy(proxy("proxy-1"));
    assert_eq!(proxy.server, "socks5://127.0.0.1:1080");
    assert_eq!(store.list_proxies().len(), 1);
    assert_eq!(store.proxy_by_id("proxy-1").unwrap().username, "user");
    assert!(store.delete_proxy("proxy-1"));
    assert!(!store.delete_proxy("proxy-1"));

    let key = store.create_api_key("Web 用户".to_string(), "secret-key-1234".to_string());
    assert_eq!(key.key_prefix, "secr");
    assert_eq!(key.key_suffix, "1234");
    assert!(store.api_key_allowed("secret-key-1234"));
    assert!(store.delete_api_key(&key.id));
    assert!(!store.api_key_allowed("secret-key-1234"));

    store.add_audit_log(
        "engine",
        "info",
        "任务完成".to_string(),
        "task-1".to_string(),
        Some("task-1".to_string()),
    );
    assert_eq!(store.list_audit_logs().len(), 1);
    store.clear_audit_logs();
    assert!(store.list_audit_logs().is_empty());
}

#[test]
fn saved_proxy_id_builds_runtime_proxy_without_copying_password_to_account() {
    let store = temp_store("runtime-proxy");
    store.create_proxy(proxy("proxy-1"));
    let mut account = account("acc-proxy", "");
    account.proxy_id = "proxy-1".to_string();
    account.proxy_url = "socks5://127.0.0.1:1080".to_string();

    let runtime = store
        .account_with_required_runtime_proxy(account.clone())
        .unwrap();

    assert_eq!(runtime.proxy_id, "proxy-1");
    assert_eq!(runtime.proxy_url, "socks5://user:pass@127.0.0.1:1080");
    assert!(store
        .account_with_required_runtime_proxy(account.clone())
        .is_ok());

    store.delete_proxy("proxy-1");
    assert!(store.account_with_required_runtime_proxy(account).is_err());
}

#[test]
fn browser_session_upsert_creates_and_relogs_accounts() {
    let store = temp_store("browser-upsert");
    let add_session = session(BrowserSessionPurpose::AddAccount, None);
    let manifest = manifest_with_default_tier();
    let created = store
        .upsert_from_browser_session_with_profile(
            &add_session,
            &manifest,
            AccountLoginProfile {
                username: "Bob".to_string(),
                email: "bob@example.test".to_string(),
                tier: String::new(),
                ..AccountLoginProfile::default()
            },
        )
        .unwrap();

    assert_eq!(created.platform, "platform-a");
    assert_eq!(created.name, "Bob");
    assert_eq!(created.email, "bob@example.test");
    assert_eq!(created.tier, "pro");
    assert_eq!(created.proxy_url, "socks5://127.0.0.1:1080");
    assert_eq!(created.proxy_id, "proxy-1");
    assert_eq!(created.browser_os, "windows");
    assert_eq!(created.browser_version, "126");

    let relogin_session = session(
        BrowserSessionPurpose::ReloginAccount,
        Some(created.id.clone()),
    );
    let relogged = store
        .upsert_from_browser_session_with_profile(
            &relogin_session,
            &manifest,
            AccountLoginProfile {
                username: "Bobby".to_string(),
                tier: "enterprise".to_string(),
                ..AccountLoginProfile::default()
            },
        )
        .unwrap();

    assert_eq!(relogged.name, "Bobby");
    assert_eq!(relogged.tier, "enterprise");
    assert!(store
        .upsert_from_browser_session(&session(BrowserSessionPurpose::DebugPreview, None))
        .is_none());
}

fn temp_store(name: &str) -> AppStore {
    let root = store_root(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    AppStore::load(store_path(name))
}

fn store_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mediagen-store-test-{name}"))
}

fn store_path(name: &str) -> std::path::PathBuf {
    store_root(name).join("server-state.json")
}

fn account(id: &str, api_key: &str) -> AccountRecord {
    account_record(id, "platform-a", id, api_key)
}

fn proxy(id: &str) -> ProxyRecord {
    ProxyRecord {
        id: id.to_string(),
        name: "住宅代理".to_string(),
        server: "socks5://127.0.0.1:1080".to_string(),
        username: "user".to_string(),
        password: "pass".to_string(),
        enabled: true,
        created_at: "2026-06-29T00:00:00Z".to_string(),
    }
}

fn session(purpose: BrowserSessionPurpose, account_id: Option<String>) -> BrowserSession {
    let mut metadata = HashMap::new();
    metadata.insert("proxy_server".to_string(), json!("socks5://127.0.0.1:1080"));
    metadata.insert("browser_os".to_string(), json!("windows"));
    metadata.insert("browser_version".to_string(), json!("126"));
    BrowserSession {
        id: "session-1".to_string(),
        purpose,
        status: BrowserSessionStatus::Active,
        platform: "platform-a".to_string(),
        account_id,
        task_id: None,
        proxy_id: Some("proxy-1".to_string()),
        start_url: "https://example.test".to_string(),
        url: "https://example.test".to_string(),
        title: String::new(),
        target_id: "target-1".to_string(),
        headless: true,
        browser_name: "CloakBrowser".to_string(),
        runtime_id: "cloakbrowser".to_string(),
        profile_dir: "/tmp/profile".to_string(),
        browser_profile_dir: "/tmp/profile".to_string(),
        viewport: BrowserViewport {
            width: 1366,
            height: 820,
        },
        device_scale_factor: 1.0,
        message: String::new(),
        cookie_count: 0,
        verification_reason: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        cancelled_at: None,
        completed_at: None,
        metadata,
    }
}

fn manifest_with_default_tier() -> PluginManifest {
    let mut manifest = PluginManifest {
        platform_id: "platform-a".to_string(),
        display_name: "Platform A".to_string(),
        sort_order: 1,
        adapter_mode: Default::default(),
        login_url: String::new(),
        workspace_url: String::new(),
        models: Vec::new(),
        tiers: Default::default(),
        flows: Default::default(),
        extra: Default::default(),
    };
    manifest.tiers.insert(
        "pro".to_string(),
        TierDef {
            text_concurrency_limit: 0,
            text_daily_limit: 0,
            text_total_limit: 0,
            display_name: "Pro".to_string(),
            is_default: true,
            concurrency_limit: 1,
            daily_limit: 0,
            total_limit: 0,
            cooldown_seconds: 0,
            task_interval_seconds: 0,
        },
    );
    manifest
}

#[test]
fn plugin_tier_controls_new_account_text_concurrency() {
    for limit in [0, 3, 100] {
        let store = AppStore::load(
            std::env::temp_dir().join(format!("tier-default-{}.sqlite", uuid::Uuid::new_v4())),
        );
        let mut manifest = manifest_with_default_tier();
        manifest
            .tiers
            .get_mut("pro")
            .unwrap()
            .text_concurrency_limit = limit;
        let account = store
            .upsert_from_browser_session_with_profile(
                &session(BrowserSessionPurpose::AddAccount, None),
                &manifest,
                AccountLoginProfile::default(),
            )
            .unwrap();
        assert_eq!(account.policy.text_concurrency_limit, limit);
        assert_eq!(account.policy.text_concurrency_default, limit);
        assert_eq!(
            crate::store_helpers::text_concurrency_limit(&account),
            limit
        );
    }
}
