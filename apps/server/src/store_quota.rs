use crate::store_models::{AccountQuotaHold, AccountRecord};
use chrono::{DateTime, Utc};

pub(crate) fn scope(kind: &str) -> &'static str {
    if kind == "text" {
        "text"
    } else {
        "media"
    }
}

/// 按能力保留独立的上游限额，另一类请求失败不能覆盖已有恢复时间。
pub(crate) fn remember(account: &mut AccountRecord) {
    if !matches!(
        account.login_state.as_str(),
        "daily_quota_exhausted" | "total_quota_exhausted"
    ) {
        return;
    }
    if let Some(kind) = account.policy.quota_task_kind.as_deref() {
        account.policy.quota_holds.insert(
            scope(kind).into(),
            AccountQuotaHold {
                state: account.login_state.clone(),
                until: account.cooldown_until.clone(),
            },
        );
    }
}

pub(crate) fn blocked(account: &AccountRecord, kind: &str) -> bool {
    account
        .policy
        .quota_holds
        .get(scope(kind))
        .is_some_and(|hold| {
            hold.until
                .as_deref()
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .is_none_or(|at| at > Utc::now())
        })
}

pub(crate) fn clear_confirmed(account: &mut AccountRecord, kind: &str) {
    account.policy.quota_holds.remove(scope(kind));
}

pub(crate) fn reset_daily(account: &mut AccountRecord) {
    account.policy.quota_holds.retain(|_, hold| {
        hold.state != "daily_quota_exhausted"
            || hold
                .until
                .as_deref()
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .is_some_and(|at| at > Utc::now())
    });
}

/// 暂时忙、每日限额或有明确恢复时间的账号可以等待；停用和无恢复期限的总限额不能。
pub(crate) fn account_can_wait_for_kind(account: &AccountRecord, kind: &str) -> bool {
    if crate::store_helpers::account_ready_for_kind(account, &[], kind) {
        return true;
    }
    let mut future = account.clone();
    let timed = account
        .cooldown_until
        .as_deref()
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .is_some();
    if (timed
        && matches!(
            future.login_state.as_str(),
            "cooldown" | "daily_quota_exhausted" | "total_quota_exhausted"
        ))
        || future.login_state == "daily_quota_exhausted"
    {
        future.login_state = "available".into();
    }
    let scope = crate::store_quota::scope(kind);
    future.policy.quota_holds.retain(|key, hold| {
        key != scope
            || (hold.state != "daily_quota_exhausted"
                && hold
                    .until
                    .as_deref()
                    .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                    .is_none())
    });
    if kind == "text" {
        future.policy.text_used_today = 0;
    } else {
        future.used_today = 0;
    }
    crate::store_helpers::account_ready_for_kind(&future, &[], kind)
}
