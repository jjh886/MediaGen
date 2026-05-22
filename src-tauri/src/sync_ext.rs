//! 同步原语扩展：屏蔽 `std::sync::Mutex` 的 PoisonError 级联。
//!
//! 背景：之前任何持锁线程 panic 都会让该 Mutex 进入 poisoned 状态，
//! 后续所有 `.lock().unwrap()` 也跟着 panic，触发"一个 bug 拖垮整个进程"的雪崩。
//! 我们绝大多数场景下数据本身不会因 panic 而陷入"逻辑不一致"——SQLite 的事务由
//! rusqlite 负责回滚；HashMap、Vec 等容器在 panic 时也只是中断当前操作。
//! 因此选择"恢复 inner guard + 打 warn"而非继续 panic。

use std::sync::{Mutex, MutexGuard};

pub trait MutexExt<T> {
    /// 获取锁；若 mutex 已被 poisoned，则记录 warn 后仍然返回内层 guard，
    /// 不让一次 panic 级联击穿整个进程的所有后续访问。
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poison| {
            tracing::warn!(
                target: "sync_ext",
                "Mutex poisoned (前序持锁线程 panic 过)，恢复 inner guard 继续运行；如频繁出现请检查前置任务 panic"
            );
            poison.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn lock_recover_works_when_not_poisoned() {
        let m = Mutex::new(42_i32);
        let g = m.lock_recover();
        assert_eq!(*g, 42);
    }

    #[test]
    fn lock_recover_returns_inner_after_poison() {
        let m = Arc::new(Mutex::new(vec![1, 2, 3]));
        let m2 = Arc::clone(&m);
        let _ = thread::spawn(move || {
            let mut g = m2.lock().unwrap();
            g.push(4);
            panic!("intentional panic to poison the mutex");
        })
        .join();
        // mutex 现在已 poisoned；普通 .lock().unwrap() 会 panic
        assert!(m.lock().is_err());
        // lock_recover 应该仍然能拿到内层数据
        let g = m.lock_recover();
        assert_eq!(*g, vec![1, 2, 3, 4]);
    }
}
