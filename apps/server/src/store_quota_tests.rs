use chrono::{Local, Utc};

use crate::store::AppStore;
use crate::test_fixtures::account_record;

#[test]
fn platform_quota_reset_time_releases_account_dispatch() {
    let store = test_store("quota-reset-release");
    let mut account = account_record("acc-1", "platform-a", "Platform A", "");
    account.used_today_date = Local::now().date_naive().to_string();
    store.insert_account(account);
    store.mark_account_state_for_seconds("acc-1", "daily_quota_exhausted", 60);
    assert!(store
        .first_available_account_for_key("platform-a", None)
        .is_none());

    {
        let mut accounts = store.accounts.lock().unwrap_or_else(|err| err.into_inner());
        accounts[0].cooldown_until = Some((Utc::now() - chrono::Duration::minutes(1)).to_rfc3339());
    }

    let account = store
        .first_available_account_for_key("platform-a", None)
        .expect("恢复时间过后账号应重新可用");
    assert_eq!(account.login_state, "available");
    assert_eq!(account.cooldown_until, None);
}

fn test_store(name: &str) -> AppStore {
    let root = std::env::temp_dir().join(format!("mediagen-store-quota-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    AppStore::load(root.join("state.json"))
}

#[test]
fn text_quota_and_cancellation_are_isolated() {
    let root = std::env::temp_dir().join(format!("quota-test-{}", uuid::Uuid::new_v4()));
    let store = AppStore::load(root.join("store.json"));
    let mut account = crate::test_fixtures::account_record("quota", "test", "Quota", "");
    account.daily_limit = 1;
    account.used_today_date = chrono::Local::now().date_naive().to_string();
    account.used_today = 1;
    account.policy.text_daily_limit = 2;
    store.insert_account(account);
    assert!(store.select_account("test", None, &[], "image").is_none());
    assert!(store.select_account("test", None, &[], "text").is_some());
    store.mark_account_usage_for("quota", "text");
    store.mark_account_usage_for("quota", "text");
    assert!(store.select_account("test", None, &[], "text").is_none());
    assert_eq!(store.account_by_id("quota").unwrap().used_today, 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn selection_obeys_priority_and_rotates_accounts() {
    let root = std::env::temp_dir().join(format!("select-test-{}", uuid::Uuid::new_v4()));
    let store = AppStore::load(root.join("store.json"));
    for id in ["a", "b"] {
        store.insert_account(crate::test_fixtures::account_record(id, "test", id, ""));
    }
    assert_eq!(
        store.select_account("test", None, &[], "text").unwrap().id,
        "a"
    );
    store.mark_account_dispatched("a");
    assert_eq!(
        store.select_account("test", None, &[], "text").unwrap().id,
        "b"
    );
    store.update_account(
        "a",
        crate::store::AccountUpdate {
            priority: Some(-1),
            ..Default::default()
        },
    );
    assert_eq!(
        store.select_account("test", None, &[], "text").unwrap().id,
        "a"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn upstream_quota_scope_and_health_checks_cannot_block_the_other_channel() {
    let store = test_store("scoped-quota");
    let mut account = account_record("scoped", "test", "Scoped", "");
    account.used_today_date = Local::now().date_naive().to_string();
    store.insert_account(account);
    store.mark_account_state("scoped", "daily_quota_exhausted");
    store.record_account_failure("scoped", "quota_exhausted", "image");
    assert!(store.select_account("test", None, &[], "text").is_some());
    assert!(store.select_account("test", None, &[], "image").is_none());
    store.mark_account_state("scoped", "daily_quota_exhausted");
    store.record_account_failure("scoped", "quota_exhausted", "text");
    assert!(store.select_account("test", None, &[], "image").is_none());
    assert!(store.select_account("test", None, &[], "text").is_none());
    let restored = AppStore::load(store.path.clone());
    assert!(restored
        .select_account("test", None, &[], "image")
        .is_none());
    assert!(restored.select_account("test", None, &[], "text").is_none());
    store.mark_login_check_result(
        "scoped",
        "available",
        Some(crate::account_login::AccountLoginProfile {
            quota_task_kind: Some("image".into()),
            ..Default::default()
        }),
    );
    assert!(store.select_account("test", None, &[], "image").is_some());
    assert!(store.select_account("test", None, &[], "text").is_none());
    store.mark_account_state_for_seconds("scoped", "daily_quota_exhausted", 1);
    let mut account = store.account_by_id("scoped").unwrap();
    account.cooldown_until = Some((Utc::now() - chrono::Duration::seconds(1)).to_rfc3339());
    store.delete_account("scoped");
    store.insert_account(account);
    assert!(store.select_account("test", None, &[], "text").is_some());
    assert!(store
        .account_by_id("scoped")
        .unwrap()
        .policy
        .quota_task_kind
        .is_none());
}
