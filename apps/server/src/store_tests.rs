use std::collections::HashMap;

use chrono::Utc;
use mediagen_browser_gateway::{
    BrowserSession, BrowserSessionPurpose, BrowserSessionStatus, BrowserViewport,
};
use mediagen_core::{PluginManifest, TierDef};
use serde_json::json;

use crate::account_login::AccountLoginProfile;
use crate::store::{
    AccountRecord, AccountUpdate, AppStore, ProxyRecord, SettingsUpdate, TaskRecord,
};
use crate::test_fixtures::account_record;

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
            .first_available_account_for_key("codex", Some("global-key"))
            .map(|item| item.id),
        Some("acc-default".to_string())
    );
    assert_eq!(
        store
            .first_available_account_for_key("codex", Some("plain-key"))
            .map(|item| item.id),
        Some("acc-default".to_string())
    );
    assert_eq!(
        store
            .first_available_account_for_key("codex", Some("account-only-key"))
            .map(|item| item.id),
        Some("acc-explicit".to_string())
    );
    assert!(store
        .first_available_account_for_key("codex", Some("unknown-key"))
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

    let runtime = store.account_with_runtime_proxy(account.clone());

    assert_eq!(runtime.proxy_id, "proxy-1");
    assert_eq!(runtime.proxy_url, "socks5://user:pass@127.0.0.1:1080");
    assert!(store
        .account_with_required_runtime_proxy(account.clone())
        .is_ok());

    store.delete_proxy("proxy-1");
    assert!(store
        .account_with_runtime_proxy(account.clone())
        .proxy_url
        .is_empty());
    assert!(store.account_with_required_runtime_proxy(account).is_err());
}

#[test]
fn task_lifecycle_updates_result_error_and_sort_order() {
    let store = temp_store("tasks");
    store.insert_task(task("task-old", "running", "2026-06-28T00:00:00Z"));
    store.insert_task(task("task-new", "running", "2026-06-29T00:00:00Z"));

    let tasks = store.list_tasks(Some("codex"));
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["task-new", "task-old",]
    );

    let completed = store
        .complete_task(
            "task-old",
            json!({ "images": [{ "url": "https://example.test/out.png" }] }),
        )
        .unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(
        completed.result_url.as_deref(),
        Some("https://example.test/out.png")
    );

    assert_eq!(
        store
            .fail_task("task-new", "boom".to_string())
            .unwrap()
            .error_message
            .as_deref(),
        Some("boom")
    );
    store.insert_task(task("task-cancel", "running", "2026-06-30T00:00:00Z"));
    assert_eq!(
        store.cancel_task("task-cancel").unwrap().status.as_str(),
        "cancelled"
    );
    assert_eq!(
        store
            .list_tasks(Some("codex"))
            .into_iter()
            .find(|task| task.id == "task-cancel")
            .unwrap()
            .error_message
            .as_deref(),
        Some("用户取消")
    );
    assert!(store.complete_task("missing", json!({})).is_none());
}

#[test]
fn task_runtime_events_usage_and_verification_are_recorded() {
    let store = temp_store("task-runtime");
    store.insert_account(account("acc-runtime", ""));
    store.insert_task(TaskRecord {
        account_id: Some("acc-runtime".to_string()),
        ..task("task-runtime", "processing", "2026-06-30T00:00:00Z")
    });

    store.add_task_event("task-runtime", "info", "开始执行".to_string());
    store.mark_account_usage("acc-runtime");
    let account = store.list_accounts(Some("codex")).remove(0);
    assert_eq!(account.used_today, 1);
    assert_eq!(account.used_total, 1);

    let verification = store
        .require_task_verification("task-runtime", "需要验证".to_string())
        .unwrap();
    assert_eq!(verification.status, "verification_required");
    assert_eq!(verification.error_message.as_deref(), Some("需要验证"));

    let resumed = store
        .resume_task_after_verification("task-runtime")
        .unwrap();
    assert_eq!(resumed.status, "queued");
    assert!(resumed.error_message.is_none());

    let reloaded = AppStore::load(store_path("task-runtime"));
    assert_eq!(reloaded.list_tasks(Some("codex"))[0].status, "queued");
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

    assert_eq!(created.platform, "codex");
    assert_eq!(created.name, "Bob");
    assert_eq!(created.email, "bob@example.test");
    assert_eq!(created.tier, "pro");
    assert_eq!(created.proxy_url, "socks5://127.0.0.1:1080");
    assert_eq!(created.proxy_id, "proxy-1");
    assert_eq!(created.browser_os, "windows");
    assert_eq!(created.browser_version, "126");

    let mut relogin_session = session(
        BrowserSessionPurpose::ReloginAccount,
        Some(created.id.clone()),
    );
    relogin_session.profile_dir = "/tmp/relogin-profile".to_string();
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
    assert_eq!(relogged.profile_dir, "/tmp/relogin-profile");
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
    account_record(id, "codex", id, api_key)
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

fn task(id: &str, status: &str, created_at: &str) -> TaskRecord {
    TaskRecord {
        id: id.to_string(),
        platform: "codex".to_string(),
        task_kind: "image".to_string(),
        model_id: "gpt-image-1".to_string(),
        prompt: "draw".to_string(),
        account_id: None,
        status: status.to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: created_at.to_string(),
        completed_at: None,
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
        platform: "codex".to_string(),
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
        platform_id: "codex".to_string(),
        display_name: "Codex".to_string(),
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
