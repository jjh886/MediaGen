use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use actix_web::web;
use tokio::sync::mpsc;

use super::image_events::sse_bytes;
use crate::scheduler::{task_interval_remaining_seconds, Scheduler};
use crate::sync_ext::MutexExt;
use crate::types::{AllocationResult, LoginState, PlatformAccount};
use crate::AppState;

pub(crate) type PrepareError = (u16, &'static str, String);

const ACCOUNT_BUSY_RETRY_DELAY: Duration = Duration::from_secs(1);
const ACCOUNT_QUEUE_REPORT_INTERVAL: Duration = Duration::from_secs(10);
const ACCOUNT_CHECK_INTERVAL_SECS: i64 = 86400;

static ACCOUNT_WAIT_QUEUES: OnceLock<Mutex<HashMap<String, VecDeque<String>>>> = OnceLock::new();

pub(crate) struct ActiveAccountLease {
    scheduler: Arc<Mutex<Scheduler>>,
    account_id: String,
    released: bool,
}

#[derive(Clone)]
pub(crate) struct AccountQueueReporter {
    tx: mpsc::Sender<bytes::Bytes>,
    request_id: String,
    event_name: String,
}

struct AccountWaitStatus {
    platform: String,
    reason: &'static str,
    ahead: usize,
    queued: usize,
    queue_position: usize,
    active: i32,
    cooldown_seconds: Option<i64>,
    wait_seconds: Option<i64>,
    message: String,
}

struct AccountWaitSlot {
    platform: String,
    request_id: String,
}

impl ActiveAccountLease {
    pub(crate) fn new(scheduler: Arc<Mutex<Scheduler>>, account_id: String) -> Self {
        Self {
            scheduler,
            account_id,
            released: false,
        }
    }

    pub(crate) fn release(&mut self) {
        if self.released {
            return;
        }
        self.scheduler
            .lock_recover()
            .decrement_active(&self.account_id);
        self.released = true;
    }
}

impl Drop for ActiveAccountLease {
    fn drop(&mut self) {
        self.release();
    }
}

impl AccountQueueReporter {
    pub(crate) fn new(
        tx: mpsc::Sender<bytes::Bytes>,
        request_id: String,
        event_name: String,
    ) -> Self {
        Self {
            tx,
            request_id,
            event_name,
        }
    }

    async fn emit(&self, status: &AccountWaitStatus) {
        let _ = self
            .tx
            .send(sse_bytes(
                &self.event_name,
                &serde_json::json!({
                    "type": self.event_name,
                    "request_id": self.request_id,
                    "platform": status.platform,
                    "reason": status.reason,
                    "ahead": status.ahead,
                    "queued": status.queued,
                    "queue_position": status.queue_position,
                    "active": status.active,
                    "cooldown_seconds": status.cooldown_seconds,
                    "wait_seconds": status.wait_seconds,
                    "message": status.message,
                }),
            ))
            .await;
    }
}

pub(crate) async fn allocate_account_when_available(
    state: &web::Data<AppState>,
    platform: &str,
    request_id: &str,
    reporter: Option<AccountQueueReporter>,
) -> Result<(String, String, String, ActiveAccountLease), PrepareError> {
    allocate_account_when_available_excluding(
        state,
        platform,
        request_id,
        reporter,
        &HashSet::new(),
    )
    .await
}

pub(crate) async fn allocate_account_when_available_excluding(
    state: &web::Data<AppState>,
    platform: &str,
    request_id: &str,
    reporter: Option<AccountQueueReporter>,
    excluded_account_ids: &HashSet<String>,
) -> Result<(String, String, String, ActiveAccountLease), PrepareError> {
    let mut logged_wait = false;
    let mut next_report_at = Instant::now();
    let mut wait_slot: Option<AccountWaitSlot> = None;

    loop {
        let wait_status = {
            let storage = state.storage.lock_recover();
            if let Err(e) = storage.recover_expired_cooldowns() {
                tracing::warn!(error = %e, "恢复冷却账号失败");
            }
            let accounts = storage
                .list_accounts_by_platform(platform)
                .map_err(|e| (500u16, "storage_error", e.to_string()))?
                .into_iter()
                .filter(|account| !excluded_account_ids.contains(&account.id))
                .collect::<Vec<_>>();
            let mut scheduler = state.scheduler.lock_recover();

            // 前面已经有人在等同一平台账号时，新请求先进入等待队列，避免后来者抢先拿走刚释放的账号。
            if wait_slot.is_none() && account_wait_queue_len(platform) > 0 {
                let (ahead, total) = AccountWaitSlot::ensure(&mut wait_slot, platform, request_id);
                Some(build_wait_status(
                    platform, &scheduler, &accounts, ahead, total,
                ))
            } else {
                match scheduler.allocate_with_reason(
                    platform,
                    &accounts,
                    ACCOUNT_CHECK_INTERVAL_SECS,
                ) {
                    AllocationResult::Allocated(id) => {
                        if wait_slot
                            .as_ref()
                            .and_then(AccountWaitSlot::position)
                            .is_some_and(|(ahead, _)| ahead > 0)
                        {
                            let (ahead, total) =
                                AccountWaitSlot::ensure(&mut wait_slot, platform, request_id);
                            Some(build_wait_status(
                                platform, &scheduler, &accounts, ahead, total,
                            ))
                        } else {
                            drop(wait_slot.take());
                            let acc = accounts.iter().find(|a| a.id == id).unwrap();
                            scheduler.increment_active(&id);
                            let lease =
                                ActiveAccountLease::new(state.scheduler.clone(), id.clone());
                            return Ok((id, acc.profile_dir.clone(), acc.cookies.clone(), lease));
                        }
                    }
                    AllocationResult::NoAccounts => {
                        return Err((
                            503u16,
                            "no_accounts",
                            format!("{} 平台没有可用账号", platform),
                        ));
                    }
                    AllocationResult::AllBusy => {
                        if can_wait_for_account_slot(&scheduler, &accounts) {
                            let (ahead, total) =
                                AccountWaitSlot::ensure(&mut wait_slot, platform, request_id);
                            Some(build_wait_status(
                                platform, &scheduler, &accounts, ahead, total,
                            ))
                        } else {
                            return Err((
                                503u16,
                                "no_available_accounts",
                                format!("{} 平台没有可排队的可用账号", platform),
                            ));
                        }
                    }
                    AllocationResult::AllQuotaExhausted => {
                        return Err((
                            503u16,
                            "quota_exhausted",
                            format!("{} 平台所有账号配额耗尽", platform),
                        ));
                    }
                }
            }
        };

        if let Some(status) = wait_status {
            if !logged_wait {
                tracing::info!(platform, "平台账号忙碌，任务进入账号等待队列");
                logged_wait = true;
            }
            let now = Instant::now();
            if let Some(reporter) = reporter.as_ref().filter(|_| now >= next_report_at) {
                reporter.emit(&status).await;
                next_report_at = now + ACCOUNT_QUEUE_REPORT_INTERVAL;
            }
            tokio::time::sleep(ACCOUNT_BUSY_RETRY_DELAY).await;
        }
    }
}

fn account_wait_queues() -> &'static Mutex<HashMap<String, VecDeque<String>>> {
    ACCOUNT_WAIT_QUEUES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn account_wait_queue_len(platform: &str) -> usize {
    account_wait_queues()
        .lock_recover()
        .get(platform)
        .map(VecDeque::len)
        .unwrap_or(0)
}

impl AccountWaitSlot {
    fn ensure(slot: &mut Option<Self>, platform: &str, request_id: &str) -> (usize, usize) {
        if slot.is_none() {
            let mut queues = account_wait_queues().lock_recover();
            let queue = queues.entry(platform.to_string()).or_default();
            if !queue.iter().any(|id| id == request_id) {
                queue.push_back(request_id.to_string());
            }
            *slot = Some(Self {
                platform: platform.to_string(),
                request_id: request_id.to_string(),
            });
        }
        slot.as_ref().and_then(Self::position).unwrap_or((0, 1))
    }

    fn position(&self) -> Option<(usize, usize)> {
        let queues = account_wait_queues().lock_recover();
        let queue = queues.get(&self.platform)?;
        let index = queue.iter().position(|id| id == &self.request_id)?;
        Some((index, queue.len()))
    }
}

impl Drop for AccountWaitSlot {
    fn drop(&mut self) {
        let mut queues = account_wait_queues().lock_recover();
        let Some(queue) = queues.get_mut(&self.platform) else {
            return;
        };
        queue.retain(|id| id != &self.request_id);
        if queue.is_empty() {
            queues.remove(&self.platform);
        }
    }
}

fn build_wait_status(
    platform: &str,
    scheduler: &Scheduler,
    accounts: &[PlatformAccount],
    ahead: usize,
    total: usize,
) -> AccountWaitStatus {
    let cooldown_seconds = next_cooldown_seconds(accounts);
    let task_interval_seconds = next_task_interval_seconds(accounts);
    let (reason, wait_seconds) = if let Some(seconds) = cooldown_seconds {
        ("cooldown", Some(seconds))
    } else if let Some(seconds) = task_interval_seconds {
        ("task_interval", Some(seconds))
    } else {
        ("busy", None)
    };
    let queued = total.saturating_sub(1);
    AccountWaitStatus {
        platform: platform.to_string(),
        reason,
        ahead,
        queued,
        queue_position: ahead + 1,
        active: accounts.iter().map(|a| scheduler.active_count(&a.id)).sum(),
        cooldown_seconds,
        wait_seconds,
        message: wait_message(reason, ahead, wait_seconds),
    }
}

fn wait_message(reason: &str, ahead: usize, wait_seconds: Option<i64>) -> String {
    let prefix = if ahead == 0 {
        "当前任务排在最前面".to_string()
    } else {
        format!("前面还有 {ahead} 个任务")
    };
    match (reason, wait_seconds) {
        ("cooldown", Some(seconds)) => format!("{prefix}，账号冷却中，约 {seconds} 秒后重试"),
        ("task_interval", Some(seconds)) => {
            format!("{prefix}，账号任务间隔中，约 {seconds} 秒后重试")
        }
        _ => format!("{prefix}，正在等待可用账号"),
    }
}

fn next_cooldown_seconds(accounts: &[PlatformAccount]) -> Option<i64> {
    let now = chrono::Utc::now();
    accounts
        .iter()
        .filter(|account| is_recoverable_cooldown(account))
        .filter_map(|account| account.cooldown_until)
        .map(|cooldown_until| (cooldown_until - now).num_seconds().max(0))
        .min()
}

fn next_task_interval_seconds(accounts: &[PlatformAccount]) -> Option<i64> {
    accounts
        .iter()
        .filter(|account| {
            account.login_state == LoginState::Available
                && !cooldown_active(account)
                && account.daily_limit_available()
                && account.total_limit_available()
                && last_check_fresh(account)
        })
        .filter_map(task_interval_remaining_seconds)
        .min()
}

fn can_wait_for_account_slot(scheduler: &Scheduler, accounts: &[PlatformAccount]) -> bool {
    accounts.iter().any(|account| {
        if !account.daily_limit_available()
            || !account.total_limit_available()
            || !last_check_fresh(account)
        {
            return false;
        }
        if is_recoverable_cooldown(account) {
            return true;
        }
        if account.login_state == LoginState::Available
            && !cooldown_active(account)
            && task_interval_remaining_seconds(account).is_some()
        {
            return true;
        }
        account.login_state == LoginState::Available
            && !cooldown_active(account)
            && account.concurrency_limit > 0
            && scheduler.active_count(&account.id) >= account.concurrency_limit
    })
}

trait AccountScheduleExt {
    fn daily_limit_available(&self) -> bool;
    fn total_limit_available(&self) -> bool;
}

impl AccountScheduleExt for PlatformAccount {
    fn daily_limit_available(&self) -> bool {
        self.daily_limit == 0 || self.used_today < self.daily_limit
    }

    fn total_limit_available(&self) -> bool {
        self.total_limit == 0 || self.used_total < self.total_limit
    }
}

fn cooldown_active(account: &PlatformAccount) -> bool {
    account
        .cooldown_until
        .is_some_and(|cd| cd > chrono::Utc::now())
}

fn is_recoverable_cooldown(account: &PlatformAccount) -> bool {
    account.login_state == LoginState::Cooldown && cooldown_active(account)
}

fn last_check_fresh(account: &PlatformAccount) -> bool {
    let Some(last) = account.last_check_at else {
        return false;
    };
    chrono::Utc::now() - last <= chrono::Duration::seconds(ACCOUNT_CHECK_INTERVAL_SECS * 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account(id: &str) -> PlatformAccount {
        crate::types::platform_account_fixture(id, "chatgpt")
    }

    #[test]
    fn busy_available_account_can_enter_wait_queue() {
        let mut scheduler = Scheduler::new();
        let account = make_account("a1");
        scheduler.increment_active("a1");

        assert!(can_wait_for_account_slot(&scheduler, &[account]));
    }

    #[test]
    fn exhausted_quota_account_cannot_enter_wait_queue() {
        let scheduler = Scheduler::new();
        let mut account = make_account("a1");
        account.daily_limit = 3;
        account.used_today = 3;

        assert!(!can_wait_for_account_slot(&scheduler, &[account]));
    }

    #[test]
    fn task_interval_account_can_enter_wait_queue() {
        let scheduler = Scheduler::new();
        let mut account = make_account("a1");
        account.task_interval_seconds = 30;
        account.last_used_at = Some(chrono::Utc::now());

        assert!(can_wait_for_account_slot(&scheduler, &[account]));
    }

    #[test]
    fn wait_status_reports_task_interval_reason() {
        let scheduler = Scheduler::new();
        let mut account = make_account("a1");
        account.task_interval_seconds = 30;
        account.last_used_at = Some(chrono::Utc::now());

        let status = build_wait_status("chatgpt", &scheduler, &[account], 0, 1);

        assert_eq!(status.reason, "task_interval");
        assert!(status.wait_seconds.is_some());
        assert!(status.message.contains("账号任务间隔中"));
    }

    #[test]
    fn dropping_account_lease_releases_active_count() {
        let scheduler = Arc::new(Mutex::new(Scheduler::new()));
        scheduler.lock_recover().increment_active("a1");

        {
            let _lease = ActiveAccountLease::new(scheduler.clone(), "a1".to_string());
        }

        assert_eq!(scheduler.lock_recover().active_count("a1"), 0);
    }
}
