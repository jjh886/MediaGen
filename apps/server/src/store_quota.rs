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
