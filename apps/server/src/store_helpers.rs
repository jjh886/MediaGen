use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use crate::account_login::AccountLoginProfile;
use crate::store_models::{
    AccountRecord, AccountUpdate, GenerationJobRecord, GenerationJobState, TaskRecord,
};

// AppStore 的纯计算辅助逻辑集中放这里，避免 store.rs 继续承担过多职责。
pub(crate) fn account_ready_for_dispatch(account: &AccountRecord, tasks: &[TaskRecord]) -> bool {
    account_ready_for_dispatch_at(account, tasks, Utc::now())
}

/// 把跨日账号恢复为新一天的初始状态，避免每日用量一直累加成总用量。
pub(crate) fn reset_daily_usage(account: &mut AccountRecord, today: &str) -> bool {
    if account.used_today_date == today {
        return false;
    }
    account.used_today = 0;
    account.used_today_date = today.to_string();
    if account.login_state == "daily_quota_exhausted" {
        account.login_state = "available".to_string();
    }
    true
}

pub(crate) fn state_releases_after_until(login_state: &str) -> bool {
    matches!(login_state, "cooldown" | "daily_quota_exhausted")
}

fn account_ready_for_dispatch_at(
    account: &AccountRecord,
    tasks: &[TaskRecord],
    now: DateTime<Utc>,
) -> bool {
    account_login_ready(account, now)
        && under_limit(account.used_today, account.daily_limit)
        && under_limit(account.used_total, account.total_limit)
        && under_concurrency(account, tasks)
        && interval_elapsed(account, tasks, now)
}

pub(crate) fn proxy_url_from_session_metadata(metadata: &HashMap<String, Value>) -> Option<String> {
    text_from_session_metadata(metadata, "proxy_server")
}

pub(crate) fn text_from_session_metadata(
    metadata: &HashMap<String, Value>,
    key: &str,
) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn first_result_url(result: &Value) -> Option<String> {
    result
        .get("images")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("url"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

pub(crate) fn apply_account_update(account: &mut AccountRecord, update: AccountUpdate) {
    apply_account_limits(account, &update);
    apply_account_proxy(account, &update);
    apply_account_browser_and_keys(account, update);
}

pub(crate) fn apply_account_profile(account: &mut AccountRecord, profile: AccountLoginProfile) {
    set_non_empty(&mut account.name, profile.username);
    set_non_empty(&mut account.email, profile.email);
    set_non_empty(&mut account.avatar_url, profile.avatar_url);
    set_non_empty(&mut account.tier, profile.tier);
}

fn apply_account_limits(account: &mut AccountRecord, update: &AccountUpdate) {
    if let Some(value) = &update.tier {
        account.tier = value.clone();
    }
    if let Some(value) = update.concurrency_limit {
        account.concurrency_limit = value;
    }
    if let Some(value) = update.daily_limit {
        account.daily_limit = value;
    }
    if let Some(value) = update.total_limit {
        account.total_limit = value;
    }
    if let Some(value) = update.task_interval_seconds {
        account.task_interval_seconds = value;
    }
}

fn apply_account_proxy(account: &mut AccountRecord, update: &AccountUpdate) {
    if let Some(value) = &update.proxy_url {
        account.proxy_url = value.clone();
        account.proxy_id.clear();
    }
    if let Some(value) = &update.proxy_id {
        account.proxy_id = value.clone();
    }
}

fn apply_account_browser_and_keys(account: &mut AccountRecord, update: AccountUpdate) {
    if let Some(value) = update.browser_os {
        account.browser_os = value;
    }
    if let Some(value) = update.browser_version {
        account.browser_version = value;
    }
    if let Some(value) = update.api_key {
        account.api_key = crate::access_control::hash_account_keys(&value);
    }
}

fn set_non_empty(target: &mut String, value: String) {
    if !value.trim().is_empty() {
        *target = value;
    }
}

fn account_login_ready(account: &AccountRecord, now: DateTime<Utc>) -> bool {
    if account.login_state == "available" {
        return true;
    }
    state_releases_after_until(&account.login_state)
        && account
            .cooldown_until
            .as_deref()
            .and_then(parse_time)
            .is_some_and(|until| until <= now)
}

fn under_limit(used: u32, limit: u32) -> bool {
    limit == 0 || used < limit
}

fn under_concurrency(account: &AccountRecord, tasks: &[TaskRecord]) -> bool {
    let running = tasks
        .iter()
        .filter(|task| task.account_id.as_deref() == Some(account.id.as_str()))
        .filter(|task| task_is_active(task))
        .count() as u32;
    running < account.concurrency_limit.max(1)
}

/// 排队或执行中的任务仍占用账号，登录检查不能在此时操作同一浏览器资料目录。
pub(crate) fn task_is_active(task: &TaskRecord) -> bool {
    matches!(task.status.as_str(), "queued" | "processing")
}

/// 服务重启后，同步接口遗留的任务已经无法继续执行，必须释放它占用的账号并发。
pub(crate) fn recover_interrupted_direct_tasks(
    tasks: &mut [TaskRecord],
    jobs: &[GenerationJobRecord],
    completed_at: &str,
) -> usize {
    let has_recoverable_job = |task_id: &str| {
        jobs.iter().any(|job| {
            job.task_id == task_id
                && matches!(
                    job.state,
                    GenerationJobState::Queued
                        | GenerationJobState::RetryWait
                        | GenerationJobState::Leased
                )
        })
    };
    let mut recovered = 0;
    for task in tasks.iter_mut().filter(|task| task_is_active(task)) {
        if has_recoverable_job(&task.id) {
            continue;
        }
        task.status = "failed".to_string();
        task.error_message = Some("MediaGen 上次退出时任务仍在执行，请重新提交".to_string());
        task.completed_at = Some(completed_at.to_string());
        recovered += 1;
    }
    recovered
}

fn interval_elapsed(account: &AccountRecord, tasks: &[TaskRecord], now: DateTime<Utc>) -> bool {
    let interval = account.task_interval_seconds;
    if interval == 0 {
        return true;
    }
    latest_task_time(account, tasks)
        .is_none_or(|last| now.signed_duration_since(last) >= Duration::seconds(interval as i64))
}

fn latest_task_time(account: &AccountRecord, tasks: &[TaskRecord]) -> Option<DateTime<Utc>> {
    tasks
        .iter()
        .filter(|task| task.account_id.as_deref() == Some(account.id.as_str()))
        .filter_map(task_dispatch_time)
        .max()
}

fn task_dispatch_time(task: &TaskRecord) -> Option<DateTime<Utc>> {
    task.completed_at
        .as_deref()
        .or(Some(task.created_at.as_str()))
        .and_then(parse_time)
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::{
        account_ready_for_dispatch_at, recover_interrupted_direct_tasks, reset_daily_usage,
    };
    use crate::store_models::{GenerationJobRecord, GenerationJobState, TaskRecord};
    use crate::test_fixtures::account_record;

    #[test]
    fn daily_usage_resets_only_when_server_date_changes() {
        let mut account = account_record("acc-1", "platform-a", "Platform A", "");
        account.used_today = 12;
        account.used_today_date = "2026-08-10".to_string();
        account.login_state = "daily_quota_exhausted".to_string();

        assert!(reset_daily_usage(&mut account, "2026-08-11"));
        assert_eq!(account.used_today, 0);
        assert_eq!(account.used_today_date, "2026-08-11");
        assert_eq!(account.login_state, "available");
        assert!(!reset_daily_usage(&mut account, "2026-08-11"));
    }

    #[test]
    fn dispatch_respects_account_task_interval() {
        let mut account = account_record("acc-1", "platform-a", "Platform A", "");
        account.task_interval_seconds = 60;
        let now = Utc.with_ymd_and_hms(2026, 7, 2, 10, 0, 0).unwrap();
        let recent = task_for_account(
            "task-recent",
            "acc-1",
            (now - Duration::seconds(30)).to_rfc3339(),
        );
        let old = task_for_account(
            "task-old",
            "acc-1",
            (now - Duration::seconds(90)).to_rfc3339(),
        );

        assert!(!account_ready_for_dispatch_at(
            &account,
            std::slice::from_ref(&recent),
            now
        ));
        assert!(account_ready_for_dispatch_at(&account, &[old], now));
        assert!(account_ready_for_dispatch_at(&account, &[], now));
    }

    #[test]
    fn dispatch_waits_until_platform_quota_reset_time() {
        let mut account = account_record("acc-1", "platform-a", "Platform A", "");
        let now = Utc.with_ymd_and_hms(2026, 8, 14, 10, 0, 0).unwrap();
        account.login_state = "daily_quota_exhausted".to_string();
        account.cooldown_until = Some((now + Duration::minutes(1)).to_rfc3339());

        assert!(!account_ready_for_dispatch_at(&account, &[], now));

        account.cooldown_until = Some((now - Duration::minutes(1)).to_rfc3339());
        assert!(account_ready_for_dispatch_at(&account, &[], now));
    }

    fn task_for_account(id: &str, account_id: &str, created_at: String) -> TaskRecord {
        TaskRecord {
            id: id.to_string(),
            platform: "platform-a".to_string(),
            task_kind: "image".to_string(),
            model_id: "model".to_string(),
            prompt: "draw".to_string(),
            account_id: Some(account_id.to_string()),
            status: "completed".to_string(),
            result_url: None,
            storage_key: None,
            error_message: None,
            result_data: None,
            created_at,
            completed_at: None,
        }
    }

    #[test]
    fn startup_fails_only_interrupted_direct_tasks() {
        let mut direct =
            task_for_account("task-direct", "acc-1", "2026-07-30T00:00:00Z".to_string());
        direct.status = "processing".to_string();
        let mut queued =
            task_for_account("task-queued", "acc-1", "2026-07-30T00:00:00Z".to_string());
        queued.status = "queued".to_string();
        let completed = task_for_account(
            "task-completed",
            "acc-1",
            "2026-07-30T00:00:00Z".to_string(),
        );
        let mut tasks = vec![direct, queued, completed];
        let jobs = vec![generation_job("task-queued")];

        let recovered = recover_interrupted_direct_tasks(&mut tasks, &jobs, "2026-07-30T01:00:00Z");

        assert_eq!(recovered, 1);
        assert_eq!(tasks[0].status, "failed");
        assert_eq!(
            tasks[0].error_message.as_deref(),
            Some("MediaGen 上次退出时任务仍在执行，请重新提交")
        );
        assert_eq!(
            tasks[0].completed_at.as_deref(),
            Some("2026-07-30T01:00:00Z")
        );
        assert_eq!(tasks[1].status, "queued");
        assert_eq!(tasks[2].status, "completed");
    }

    fn generation_job(task_id: &str) -> GenerationJobRecord {
        GenerationJobRecord {
            task_id: task_id.to_string(),
            owner_key: "owner".to_string(),
            task_kind: "image".to_string(),
            request_json: "{}".to_string(),
            credential_ciphertext: "cipher".to_string(),
            state: GenerationJobState::Queued,
            priority: 100,
            queue_seq: 1,
            next_attempt_at: "2026-07-30T00:00:00Z".to_string(),
            retry_count: 0,
            last_failure_category: None,
            last_error_summary: None,
            lease_until: None,
            created_at: "2026-07-30T00:00:00Z".to_string(),
            updated_at: "2026-07-30T00:00:00Z".to_string(),
            started_at: None,
            completed_at: None,
        }
    }
}
