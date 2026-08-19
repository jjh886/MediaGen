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
