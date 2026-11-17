use super::{refresh_interval, run_scheduled_refresh, wait_for_schedule, AccountRefreshRuntime};
use crate::test_fixtures::{account_record, server_state};
use mediagen_plugin_runtime::PluginLoader;
use std::time::Duration;

#[actix_web::test]
async fn exhausted_shared_quota_waits_until_reset_and_other_accounts_keep_dispatching() {
    let root = tempfile::tempdir().unwrap();
    let state = server_state(root.path().into(), PluginLoader::empty());
    state
        .store
        .insert_account(account_record("limited", "test", "Limited", ""));
    state
        .store
        .insert_account(account_record("healthy", "test", "Healthy", ""));
    super::apply_refresh_outcome(
        &state,
        "limited",
        crate::account_login_check::AccountLoginCheckOutcome::Unavailable {
            login_state: "cooldown".into(),
            profile: crate::account_login::profile_from_value(&serde_json::json!({
                "logged_in":true,"quota_windows":[{"name":"5 小时额度","remaining_percent":0}]
            })),
            cooldown_seconds: Some(3600),
            quota_task_kind: None,
        },
        &mut super::AccountRefreshSummary::default(),
    );
    for kind in ["text", "image", "video"] {
        assert_eq!(
            state
                .store
                .select_account_for_task("test", None, &[], kind, None)
                .unwrap()
                .id,
            "healthy"
        );
        let account = state.store.account_by_id("limited").unwrap();
        assert!(!crate::store_helpers::account_ready_for_kind(
            &account,
            &[],
            kind
        ));
        assert!(crate::store_quota::account_can_wait_for_kind(
            &account, kind
        ));
    }
    assert_eq!(
        state
            .store
            .account_by_id("limited")
            .unwrap()
            .policy
            .quota_windows[0]["remaining_percent"],
        0
    );
    state
        .store
        .accounts
        .lock()
        .unwrap()
        .iter_mut()
        .find(|a| a.id == "limited")
        .unwrap()
        .cooldown_until = Some((chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339());
    state.store.release_expired_cooldowns();
    let recovered = state.store.account_by_id("limited").unwrap();
    assert_eq!(recovered.login_state, "available");
    assert!(crate::store_helpers::account_ready_for_kind(
        &recovered,
        &[],
        "text"
    ));
}

#[test]
fn queued_binding_does_not_occupy_account_but_live_executor_does() {
    let state = server_state(
        std::env::temp_dir().join(format!("busy-quota-{}", uuid::Uuid::new_v4())),
        PluginLoader::empty(),
    );
    let account = account_record("account", "test", "Test", "");
    state.store.insert_account(account.clone());
    let task = serde_json::from_value(serde_json::json!({
        "id":"waiting", "platform":"test", "task_kind":"image", "model_id":"image",
        "prompt":"test", "account_id":"account", "status":"queued", "created_at":"2026-01-01T00:00:00Z"
    })).unwrap();
    state.store.insert_task(task);
    assert!(super::busy_account_ids(&state).is_empty());
    let lease = crate::protocol_task::Lease::new(state.store.clone(), "waiting".into());
    assert!(super::busy_account_ids(&state).contains("account"));
    state.store.cancel_task("waiting");
    assert!(super::busy_account_ids(&state).contains("account"));
    drop(lease);
    assert!(super::busy_account_ids(&state).is_empty());
}

#[test]
fn zero_disables_refresh_and_minutes_convert_to_duration() {
    assert_eq!(refresh_interval(0), None);
    assert_eq!(refresh_interval(2), Some(Duration::from_secs(120)));
}

#[tokio::test]
async fn schedule_change_wakes_disabled_worker() {
    let runtime = AccountRefreshRuntime::default();
    runtime.notify_schedule_changed();
    assert!(!wait_for_schedule(&runtime, None).await);
}

#[tokio::test]
async fn elapsed_interval_requests_refresh() {
    let runtime = AccountRefreshRuntime::default();
    assert!(wait_for_schedule(&runtime, Some(Duration::from_millis(1))).await);
}

#[tokio::test]
async fn elapsed_schedule_runs_real_account_refresh() {
    let root = std::env::temp_dir().join("mediagen-account-auto-refresh-test");
    let _ = std::fs::remove_dir_all(&root);
    let state = server_state(root, PluginLoader::empty());
    state.store.insert_account(account_record(
        "acc-auto",
        "missing-plugin",
        "自动检查账号",
        "",
    ));

    assert!(run_scheduled_refresh(&state, Some(Duration::from_millis(1))).await);
    assert_eq!(state.store.list_audit_logs()[0].summary, "账号状态自动刷新");
}
#[tokio::test]
async fn unavailable_bound_proxy_skips_login_without_network_fallback() {
    let root = std::env::temp_dir().join(format!("proxy-refresh-{}", uuid::Uuid::new_v4()));
    let state = server_state(root, PluginLoader::empty());
    let mut account = account_record("bound", "test", "Bound", "");
    account.proxy_id = "deleted".into();
    state.store.insert_account(account);
    let summary = super::refresh_accounts(&state).await;
    assert_eq!(summary.inconclusive, 1);
    assert_eq!(summary.checked, 0);
    assert_eq!(
        state.store.account_by_id("bound").unwrap().login_state,
        "available"
    );
    assert!(state.store.list_audit_logs()[0]
        .summary
        .contains("代理不可用"));
}
