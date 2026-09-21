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
        if state.store.settings().check_interval_minutes > 0 {
            let summary = refresh_accounts(&state).await;
            record_automatic_refresh(&state.store, &summary);
        }
        loop {
            let interval =
                refresh_interval(state.store.settings().check_interval_minutes).map(|interval| {
                    state
                        .store
                        .list_accounts(None)
                        .iter()
                        .flat_map(|a| {
                            a.cooldown_until.iter().chain(
                                a.policy
                                    .quota_holds
                                    .values()
                                    .filter_map(|hold| hold.until.as_ref()),
                            )
                        })
                        .filter_map(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
                        .filter_map(|at| {
                            let seconds = at.timestamp() - chrono::Utc::now().timestamp();
                            (seconds > 0).then(|| Duration::from_secs(seconds as u64 + 1))
                        })
                        .min()
                        .map(|reset| interval.min(reset))
                        .unwrap_or(interval)
                });
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

pub(crate) async fn refresh_quota(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> actix_web::HttpResponse {
    let _guard = state.account_refresh.refresh_lock.lock().await;
    let id = path.into_inner();
    let Some(account) = state.store.account_by_id(&id) else {
        return actix_web::HttpResponse::NotFound().json(serde_json::json!({"error":"账号不存在"}));
    };
    if account.login_state == "disabled" || busy_account_ids(&state).contains(&id) {
        return actix_web::HttpResponse::Conflict()
            .json(serde_json::json!({"error":"账号已停用或正在执行任务，请稍后刷新额度"}));
    }
    let account = match state.store.account_with_required_runtime_proxy(account) {
        Ok(account) => account,
        Err(_) => {
            return actix_web::HttpResponse::Conflict()
                .json(serde_json::json!({"error":"账号绑定的代理不可用"}))
        }
    };
    let outcome =
        crate::account_login_check::check_saved_account_login_with_options(&state, &account, true)
            .await;
    if let AccountLoginCheckOutcome::Inconclusive(_) = &outcome {
        return actix_web::HttpResponse::BadGateway()
            .json(serde_json::json!({"error":"额度暂时无法确认，已保留原数据，请稍后重试"}));
    }
    apply_refresh_outcome(&state, &id, outcome, &mut AccountRefreshSummary::default());
    state.account_refresh.notify_schedule_changed();
    match state.store.account_by_id(&id) {
        Some(account) => actix_web::HttpResponse::Ok()
            .json(serde_json::json!({"data": crate::routes::public_account(account)})),
        None => actix_web::HttpResponse::NotFound().finish(),
    }
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
    let leases = state
        .store
        .protocol_leases
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    state
        .store
        .list_tasks(None)
        .into_iter()
        .filter(|task| task_is_active(task) || leases.contains(&task.id))
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
    let runtime_account = match state
        .store
        .account_with_required_runtime_proxy(account.clone())
    {
        Ok(account) => account,
        Err(_) => {
            summary.inconclusive += 1;
            state.store.add_audit_log(
                "account",
                "warn",
                "账号检查已跳过：绑定代理不可用".into(),
                format!("account_id={}", account.id),
                None,
            );
            return;
        }
    };
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
    if state
        .store
        .account_by_id(account_id)
        .is_none_or(|account| account.login_state == "disabled")
    {
        return;
    }
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
            quota_task_kind,
        } => {
            apply_unavailable_state(state, account_id, &login_state, profile, cooldown_seconds);
            if let Some(kind) = quota_task_kind {
                state
                    .store
                    .record_account_failure(account_id, &login_state, &kind);
            }
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
}

#[cfg(test)]
mod quota_tests {
    use actix_web::{test as at, web, App};
    use serde_json::json;
    #[actix_web::test]
    async fn manual_quota_refresh_passes_flag_and_persists_plugin_windows() {
        let root = std::env::temp_dir().join(format!("manual-quota-{}", uuid::Uuid::new_v4()));
        let plugins = root.join("plugins");
        std::fs::create_dir_all(plugins.join("test/scripts")).unwrap();
        std::fs::write(
            plugins.join("plugins.index.json"),
            r#"{"plugins":["test"]}"#,
        )
        .unwrap();
        std::fs::write(plugins.join("test/manifest.json"),json!({"platform_id":"test","display_name":"Test","adapter_mode":"api","flows":{"check_account":{"steps":[{"action":"plugin.run_js","script_file":"quota.js"}]}}}).to_string()).unwrap();
        std::fs::write(plugins.join("test/scripts/quota.js"), "function main(ctx){if(!ctx.refresh_quota)throw new Error('manual flag missing');return {status:'available',logged_in:true,quota_task_kind:'text',quota_windows:[{name:'week',remaining_percent:40,task_kind:'text'}]}}").unwrap();
        let state = crate::test_fixtures::server_state(
            root.clone(),
            mediagen_plugin_runtime::PluginLoader::load_from(&plugins).unwrap(),
        );
        state
            .store
            .insert_account(crate::test_fixtures::account_record(
                "quota", "test", "Quota", "",
            ));
        let app = at::init_service(
            App::new()
                .app_data(web::Data::new(state.clone()))
                .route("/refresh/{id}", web::post().to(super::refresh_quota)),
        )
        .await;
        let response = at::call_service(
            &app,
            at::TestRequest::post().uri("/refresh/quota").to_request(),
        )
        .await;
        assert_eq!(response.status(), 200);
        let body: serde_json::Value = at::read_body_json(response).await;
        assert_eq!(body["data"]["quota_windows"][0]["remaining_percent"], 40);
        state.store.toggle_account("quota", true);
        assert_eq!(
            at::call_service(
                &app,
                at::TestRequest::post().uri("/refresh/quota").to_request()
            )
            .await
            .status(),
            409
        );
        assert_eq!(
            at::call_service(
                &app,
                at::TestRequest::post().uri("/refresh/missing").to_request()
            )
            .await
            .status(),
            404
        );
    }
}
