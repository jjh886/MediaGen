use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{Duration, Utc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

use crate::types::{AllocationResult, LoginState, PlatformAccount};

pub struct TaskQueue {
    semaphore: Arc<Semaphore>,
    queued_count: AtomicUsize,
    max_concurrent: usize,
    max_queued: usize,
}

pub struct TaskQueuePermit {
    _permit: OwnedSemaphorePermit,
}

#[derive(Debug, Clone)]
pub enum TaskQueueError {
    QueueFull { queued: usize, max_queued: usize },
    Closed,
}

impl fmt::Display for TaskQueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskQueueError::QueueFull { queued, max_queued } => {
                write!(
                    f,
                    "图片生成等待队列已满（当前等待 {queued}，上限 {max_queued}）"
                )
            }
            TaskQueueError::Closed => write!(f, "图片生成队列已关闭"),
        }
    }
}

impl TaskQueue {
    pub fn new(max_concurrent: usize, max_queued: usize) -> Self {
        let max_concurrent = max_concurrent.max(1);
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            queued_count: AtomicUsize::new(0),
            max_concurrent,
            max_queued,
        }
    }

    pub async fn acquire(&self) -> Result<TaskQueuePermit, TaskQueueError> {
        match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => return Ok(TaskQueuePermit { _permit: permit }),
            Err(TryAcquireError::NoPermits) => {}
            Err(TryAcquireError::Closed) => return Err(TaskQueueError::Closed),
        }

        let _slot = self.reserve_queue_slot()?;
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| TaskQueueError::Closed)?;
        Ok(TaskQueuePermit { _permit: permit })
    }

    pub fn queued_count(&self) -> usize {
        self.queued_count.load(Ordering::Acquire)
    }

    pub fn active_count(&self) -> usize {
        self.max_concurrent
            .saturating_sub(self.semaphore.available_permits())
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    fn reserve_queue_slot(&self) -> Result<QueueSlot<'_>, TaskQueueError> {
        loop {
            let current = self.queued_count.load(Ordering::Acquire);
            if current >= self.max_queued {
                return Err(TaskQueueError::QueueFull {
                    queued: current,
                    max_queued: self.max_queued,
                });
            }
            if self
                .queued_count
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(QueueSlot {
                    queue: &self.queued_count,
                });
            }
        }
    }
}

struct QueueSlot<'a> {
    queue: &'a AtomicUsize,
}

impl Drop for QueueSlot<'_> {
    fn drop(&mut self) {
        self.queue.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct Scheduler {
    pointers: HashMap<String, usize>,
    active_counts: HashMap<String, i32>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            pointers: HashMap::new(),
            active_counts: HashMap::new(),
        }
    }

    /// 从指定平台的账号池中按 round-robin 分配一个可用账号，返回 account_id。
    pub fn allocate(
        &mut self,
        platform: &str,
        accounts: &[PlatformAccount],
        check_interval_secs: i64,
    ) -> Option<String> {
        if accounts.is_empty() {
            return None;
        }

        let len = accounts.len();
        // 默认指针为 len-1，使首次调用从 index 0 开始扫描
        let start_ptr = *self.pointers.get(platform).unwrap_or(&(len - 1));
        let start = (start_ptr + 1) % len;

        for i in 0..len {
            let idx = (start + i) % len;
            let account = &accounts[idx];
            let active = self.active_count(&account.id);

            if Self::can_schedule(account, active, check_interval_secs) {
                self.pointers.insert(platform.to_string(), idx);
                return Some(account.id.clone());
            }
        }

        None
    }

    /// 带原因诊断的调度：区分"没账号"、"全忙"和"全部配额耗尽"
    pub fn allocate_with_reason(
        &mut self,
        platform: &str,
        accounts: &[PlatformAccount],
        check_interval_secs: i64,
    ) -> AllocationResult {
        if accounts.is_empty() {
            return AllocationResult::NoAccounts;
        }

        // 先尝试正常分配（注意：cooldown 恢复在调用方传入 accounts 前由 storage 层完成）
        if let Some(id) = self.allocate(platform, accounts, check_interval_secs) {
            return AllocationResult::Allocated(id);
        }

        // 分析失败原因：是全部配额耗尽，还是暂时不可用
        let all_quota_exhausted = accounts.iter().all(|a| {
            a.login_state == LoginState::TotalQuotaExhausted
                || a.login_state == LoginState::DailyQuotaExhausted
                || (a.total_limit > 0 && a.used_total >= a.total_limit)
                || (a.daily_limit > 0 && a.used_today >= a.daily_limit)
        });

        if all_quota_exhausted {
            AllocationResult::AllQuotaExhausted
        } else {
            AllocationResult::AllBusy
        }
    }

    pub fn increment_active(&mut self, account_id: &str) {
        *self
            .active_counts
            .entry(account_id.to_string())
            .or_insert(0) += 1;
    }

    pub fn decrement_active(&mut self, account_id: &str) {
        let count = self
            .active_counts
            .entry(account_id.to_string())
            .or_insert(0);
        *count = (*count - 1).max(0);
    }

    pub fn active_count(&self, account_id: &str) -> i32 {
        self.active_counts.get(account_id).copied().unwrap_or(0)
    }

    /// 检查账号是否满足全部 6 个调度条件。
    pub fn can_schedule(account: &PlatformAccount, active: i32, check_interval_secs: i64) -> bool {
        // ① login_state == Available
        if account.login_state != LoginState::Available {
            return false;
        }
        // ② active_tasks < concurrency_limit
        if active >= account.concurrency_limit {
            return false;
        }
        // ③ used_today < daily_limit（0 表示无限制）
        if account.daily_limit != 0 && account.used_today >= account.daily_limit {
            return false;
        }
        // ④ used_total < total_limit（0 表示无限制）
        if account.total_limit != 0 && account.used_total >= account.total_limit {
            return false;
        }
        // ⑤ cooldown_until 为 None 或已过期
        if let Some(cd) = account.cooldown_until {
            if cd > Utc::now() {
                return false;
            }
        }
        // ⑥ last_check_at 存在且在 2× check_interval 以内
        match account.last_check_at {
            None => return false,
            Some(last) => {
                let threshold = Duration::seconds(check_interval_secs * 2);
                if Utc::now() - last > threshold {
                    return false;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn make_test_account(id: &str, platform: &str) -> PlatformAccount {
        let now = Utc::now();
        PlatformAccount {
            id: id.to_string(),
            platform: platform.to_string(),
            name: String::new(),
            notes: String::new(),
            login_state: LoginState::Available,
            login_state_reason: String::new(),
            login_state_updated_at: now,
            cookies: String::new(),
            api_token: String::new(),
            refresh_token: String::new(),
            token_expires_at: None,
            profile_dir: String::new(),
            tier: String::new(),
            concurrency_limit: 2,
            daily_limit: 0,
            total_limit: 0,
            used_today: 0,
            used_total: 0,
            usage_date: String::new(),
            cooldown_until: None,
            user_id: String::new(),
            username: String::new(),
            email: String::new(),
            avatar_url: String::new(),
            password_encrypted: String::new(),
            created_at: now,
            last_check_at: Some(now),
            last_refreshed_at: None,
            last_used_at: None,
        }
    }

    #[test]
    fn test_allocate_returns_available() {
        let mut sched = Scheduler::new();
        let accounts = vec![make_test_account("a1", "doubao")];
        let result = sched.allocate("doubao", &accounts, 300);
        assert_eq!(result, Some("a1".to_string()));
    }

    #[test]
    fn test_allocate_skips_expired() {
        let mut sched = Scheduler::new();
        let mut acc = make_test_account("a1", "doubao");
        acc.login_state = LoginState::LoginExpired;
        let good = make_test_account("a2", "doubao");
        let accounts = vec![acc, good];
        let result = sched.allocate("doubao", &accounts, 300);
        assert_eq!(result, Some("a2".to_string()));
    }

    #[test]
    fn test_allocate_skips_quota_exhausted() {
        let mut sched = Scheduler::new();

        // daily quota exhausted
        let mut daily = make_test_account("d1", "doubao");
        daily.daily_limit = 10;
        daily.used_today = 10;

        // total quota exhausted
        let mut total = make_test_account("t1", "doubao");
        total.total_limit = 100;
        total.used_total = 100;

        let good = make_test_account("ok", "doubao");
        let accounts = vec![daily, total, good];
        let result = sched.allocate("doubao", &accounts, 300);
        assert_eq!(result, Some("ok".to_string()));
    }

    #[test]
    fn test_round_robin_rotates() {
        let mut sched = Scheduler::new();
        let accounts = vec![
            make_test_account("a1", "doubao"),
            make_test_account("a2", "doubao"),
            make_test_account("a3", "doubao"),
        ];

        let r1 = sched.allocate("doubao", &accounts, 300);
        assert_eq!(r1, Some("a1".to_string()));

        let r2 = sched.allocate("doubao", &accounts, 300);
        assert_eq!(r2, Some("a2".to_string()));

        let r3 = sched.allocate("doubao", &accounts, 300);
        assert_eq!(r3, Some("a3".to_string()));

        // 回到 a1
        let r4 = sched.allocate("doubao", &accounts, 300);
        assert_eq!(r4, Some("a1".to_string()));
    }

    #[test]
    fn test_allocate_none_when_all_unavailable() {
        let mut sched = Scheduler::new();
        let mut acc = make_test_account("a1", "doubao");
        acc.login_state = LoginState::Disabled;
        let accounts = vec![acc];
        let result = sched.allocate("doubao", &accounts, 300);
        assert_eq!(result, None);
    }

    #[test]
    fn test_concurrency_tracking() {
        let mut sched = Scheduler::new();
        assert_eq!(sched.active_count("a1"), 0);

        sched.increment_active("a1");
        sched.increment_active("a1");
        assert_eq!(sched.active_count("a1"), 2);

        sched.decrement_active("a1");
        assert_eq!(sched.active_count("a1"), 1);

        // 不能低于 0
        sched.decrement_active("a1");
        sched.decrement_active("a1");
        assert_eq!(sched.active_count("a1"), 0);
    }

    #[test]
    fn test_allocate_with_reason_daily_exhausted_reports_quota() {
        // 当所有账号都因 daily 用量耗尽不可调度时，应报 AllQuotaExhausted 而不是 AllBusy。
        let mut sched = Scheduler::new();
        let mut a1 = make_test_account("a1", "doubao");
        a1.daily_limit = 10;
        a1.used_today = 10;
        let mut a2 = make_test_account("a2", "doubao");
        a2.daily_limit = 5;
        a2.used_today = 5;
        let accounts = vec![a1, a2];
        let result = sched.allocate_with_reason("doubao", &accounts, 300);
        assert!(matches!(result, AllocationResult::AllQuotaExhausted));
    }

    #[test]
    fn test_cooldown_check() {
        // 冷却中的账号不可调度
        let mut cooling = make_test_account("c1", "doubao");
        cooling.cooldown_until = Some(Utc::now() + Duration::minutes(10));
        assert!(!Scheduler::can_schedule(&cooling, 0, 300));

        // 冷却已过期的账号可调度
        let mut cooled = make_test_account("c2", "doubao");
        cooled.cooldown_until = Some(Utc::now() - Duration::minutes(1));
        assert!(Scheduler::can_schedule(&cooled, 0, 300));
    }
}
