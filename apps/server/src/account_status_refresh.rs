use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use actix_web::web;
use serde::Serialize;
use tokio::sync::{Mutex, Notify};

use crate::account_login_check::AccountLoginCheckOutcome;
use crate::store::{AccountRecord, AppStore};
use crate::store_helpers::task_is_active;
use crate::ServerState;

#[derive(Default)]
pub(crate) struct AccountRefreshRuntime {
    schedule_changed: Notify,
    refresh_lock: Mutex<()>,
}

impl AccountRefreshRuntime {
    pub(crate) fn notify_schedule_changed(&self) {
        self.schedule_changed.notify_one();
    }
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct AccountRefreshSummary {
    checked: usize,
    expired: usize,
    inconclusive: usize,
    limited: usize,
    skipped_busy: usize,
}

pub(crate) fn start_worker(state: web::Data<ServerState>) {
    actix_web::rt::spawn(async move {
        loop {
            let interval = refresh_interval(state.store.settings().check_interval_minutes);
            run_scheduled_refresh(&state, interval).await;
        }
    });
}

async fn run_scheduled_refresh(state: &ServerState, interval: Option<Duration>) -> bool {
    if !wait_for_schedule(&state.account_refresh, interval).await {
        return false;
    }
    let summary = refresh_accounts(state).await;
    record_automatic_refresh(&state.store, &summary);
    true
}

/// 手动刷新和定时刷新共用同一把锁，避免同时打开两组账号检查会话。
pub(crate) async fn refresh_accounts(state: &ServerState) -> AccountRefreshSummary {
    let _guard = state.account_refresh.refresh_lock.lock().await;
    let busy_accounts = busy_account_ids(state);
    let mut summary = AccountRefreshSummary::default();
    for account in state.store.list_accounts(None) {
        refresh_one_account(state, &busy_accounts, account, &mut summary).await;
    }
    summary
}

fn refresh_interval(minutes: u32) -> Option<Duration> {
    (minutes > 0).then(|| Duration::from_secs(u64::from(minutes) * 60))
}

/// 返回 true 表示间隔到期，false 表示设置变化，需要按新配置重新计时。
async fn wait_for_schedule(runtime: &AccountRefreshRuntime, interval: Option<Duration>) -> bool {
    let Some(interval) = interval else {
        runtime.schedule_changed.notified().await;
        return false;
    };
    tokio::select! {
        _ = tokio::time::sleep(interval) => true,
        _ = runtime.schedule_changed.notified() => false,
    }
}

fn busy_account_ids(state: &ServerState) -> HashSet<String> {
    state
        .store
        .list_tasks(None)
        .into_iter()
        .filter(task_is_active)
        .filter_map(|task| task.account_id)
        .collect()
}

async fn refresh_one_account(
    state: &ServerState,
    busy_accounts: &HashSet<String>,
    account: AccountRecord,
    summary: &mut AccountRefreshSummary,
) {
    if account.login_state == "disabled" {
        return;
    }
    if busy_accounts.contains(&account.id) {
        summary.skipped_busy += 1;
        return;
    }
    let runtime_account = state.store.account_with_runtime_proxy(account.clone());
    let outcome =
        crate::account_login_check::check_saved_account_login(state, &runtime_account).await;
    apply_refresh_outcome(state, &account.id, outcome, summary);
}

fn apply_refresh_outcome(
    state: &ServerState,
    account_id: &str,
    outcome: AccountLoginCheckOutcome,
    summary: &mut AccountRefreshSummary,
) {
    summary.checked += 1;
    match outcome {
        AccountLoginCheckOutcome::Available(profile) => {
            state
                .store
                .mark_login_check_result(account_id, "available", Some(profile));
        }
        AccountLoginCheckOutcome::Expired(_reason) => {
            summary.expired += 1;
            state
                .store
                .mark_login_check_result(account_id, "login_expired", None);
        }
        AccountLoginCheckOutcome::Unavailable {
            login_state,
            profile,
            cooldown_seconds,
        } => {
            apply_unavailable_state(state, account_id, &login_state, profile, cooldown_seconds);
            if account_limit_state(&login_state) {
                summary.limited += 1;
            }
        }
        AccountLoginCheckOutcome::Inconclusive(_reason) => summary.inconclusive += 1,
    }
}

fn apply_unavailable_state(
    state: &ServerState,
    account_id: &str,
    login_state: &str,
    profile: crate::account_login::AccountLoginProfile,
    cooldown_seconds: Option<i64>,
) {
    state
        .store
        .mark_login_check_result(account_id, "available", Some(profile));
    if let Some(seconds) = cooldown_seconds {
        state
            .store
            .mark_account_state_for_seconds(account_id, login_state, seconds);
    } else {
        state.store.mark_account_state(account_id, login_state);
    }
}

fn account_limit_state(login_state: &str) -> bool {
    matches!(
        login_state,
        "cooldown" | "daily_quota_exhausted" | "total_quota_exhausted"
    )
}

fn record_automatic_refresh(store: &Arc<AppStore>, summary: &AccountRefreshSummary) {
    if summary.checked == 0 && summary.skipped_busy == 0 {
        return;
    }
    store.add_audit_log(
        "account",
        "info",
        "账号状态自动刷新".to_string(),
        format!(
            "checked={}, expired={}, limited={}, inconclusive={}, skipped_busy={}",
            summary.checked,
            summary.expired,
            summary.limited,
            summary.inconclusive,
            summary.skipped_busy
        ),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        refresh_interval, run_scheduled_refresh, wait_for_schedule, AccountRefreshRuntime,
    };
    use crate::test_fixtures::{account_record, server_state};
    use mediagen_plugin_runtime::PluginLoader;
    use std::time::Duration;

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
}
