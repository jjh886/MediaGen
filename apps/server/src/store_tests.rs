use std::collections::HashMap;

use chrono::Utc;
use mediagen_browser_gateway::{
    BrowserSession, BrowserSessionPurpose, BrowserSessionStatus, BrowserViewport,
};
use mediagen_core::{PluginManifest, TierDef};
use serde_json::json;

use crate::account_login::AccountLoginProfile;
use crate::store::{AccountRecord, AccountUpdate, AppStore, SettingsUpdate, TaskRecord};

#[test]
fn api_key_selects_accounts_by_group_or_explicit_key() {
    let store = temp_store("key-groups");
    store.update_settings(SettingsUpdate {
        api_keys: Some("group-key=vip\nplain-key".to_string()),
        ..SettingsUpdate::default()
    });
    store.insert_account(account("acc-default", "默认分组", ""));
    store.insert_account(account("acc-vip", "vip", ""));
    store.insert_account(account("acc-explicit", "默认分组", "plain-key"));

    assert_eq!(
        store
            .first_available_account_for_key("codex", Some("group-key"))
            .map(|item| item.id),
        Some("acc-vip".to_string())
    );
    assert_eq!(
        store
            .first_available_account_for_key("codex", Some("plain-key"))
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
    let settings = store.update_settings(SettingsUpdate {
        api_keys: Some(" key-a = vip \n\n key-b ".to_string()),
        image_task_concurrency: Some(8),
        flow_mirror_url: Some(" https://mirror.example.com ".to_string()),
        ..SettingsUpdate::default()
    });
    assert_eq!(settings.api_keys, "key-a = vip\nkey-b");
    assert_eq!(settings.image_task_concurrency, 8);
    assert_eq!(settings.flow_mirror_url, "https://mirror.example.com");

    store.insert_account(account("acc-1", "默认分组", "key-a,key-b"));
    let updated = store
        .update_account(
            "acc-1",
            AccountUpdate {
                tier: Some("pro".to_string()),
                concurrency_limit: Some(3),
                daily_limit: Some(20),
                total_limit: Some(200),
                task_interval_seconds: Some(9),
                group_name: Some("vip".to_string()),
                proxy_url: Some("socks5://127.0.0.1:1080".to_string()),
                api_key: Some(" key-c, key-d\nkey-e ".to_string()),
            },
        )
        .unwrap();

    assert_eq!(updated.tier, "pro");
    assert_eq!(updated.concurrency_limit, 3);
    assert_eq!(updated.group_name, "vip");
    assert_eq!(updated.api_key, "key-c\nkey-d\nkey-e");

    let reloaded = AppStore::load(store_path("settings-persist"));
    assert_eq!(reloaded.settings().api_keys, "key-a = vip\nkey-b");
    assert_eq!(reloaded.settings().image_task_concurrency, 8);
    assert_eq!(
        reloaded.list_accounts(None)[0].proxy_url,
        "socks5://127.0.0.1:1080"
    );
}

#[test]
fn account_toggle_delete_and_login_check_update_state() {
    let store = temp_store("account-state");
    store.insert_account(account("acc-1", "默认分组", ""));

    assert_eq!(
        store.toggle_account("acc-1", true).unwrap().login_state,
        "disabled"
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
        store
            .cancel_task("task-cancel")
            .unwrap()
            .error_message
            .as_deref(),
        Some("用户取消")
    );
    assert!(store.complete_task("missing", json!({})).is_none());
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

fn account(id: &str, group_name: &str, api_key: &str) -> AccountRecord {
    AccountRecord {
        id: id.to_string(),
        platform: "codex".to_string(),
        name: id.to_string(),
        email: String::new(),
        avatar_url: String::new(),
        tier: "free".to_string(),
        login_state: "available".to_string(),
        used_today: 0,
        daily_limit: 100,
        used_total: 0,
        total_limit: 0,
        concurrency_limit: 1,
        task_interval_seconds: 0,
        group_name: group_name.to_string(),
        proxy_url: String::new(),
        api_key: api_key.to_string(),
        profile_dir: String::new(),
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
    BrowserSession {
        id: "session-1".to_string(),
        purpose,
        status: BrowserSessionStatus::Active,
        platform: "codex".to_string(),
        account_id,
        task_id: None,
        proxy_id: None,
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
