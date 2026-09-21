use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Clone, Default)]
pub(crate) struct PluginLocks(Arc<Mutex<HashMap<String, OwnedMutexGuard<()>>>>);
type LockMap = Mutex<HashMap<String, Weak<AsyncMutex<()>>>>;
static LOCKS: OnceLock<LockMap> = OnceLock::new();
impl PluginLocks {
    pub(crate) async fn acquire(&self, key: &str) -> Result<(), String> {
        if key.is_empty() || key.len() > 4096 {
            return Err("锁标识无效".into());
        }
        if self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(key)
        {
            return Err("插件锁不允许递归获取".into());
        }
        let lock = {
            let mut locks = LOCKS
                .get_or_init(Mutex::default)
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            locks.retain(|_, lock| lock.strong_count() > 0);
            let lock = locks
                .get(key)
                .and_then(Weak::upgrade)
                .unwrap_or_else(|| Arc::new(AsyncMutex::new(())));
            locks.insert(key.into(), Arc::downgrade(&lock));
            lock
        };
        let guard = lock.lock_owned().await;
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(key.into(), guard);
        Ok(())
    }
    pub(crate) fn release(&self, key: &str) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn account_lock_is_released_when_plugin_exits() {
        let key = format!("lock-{}", uuid::Uuid::new_v4());
        let one = PluginLocks::default();
        let two = PluginLocks::default();
        one.acquire(&key).await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), two.acquire(&key))
                .await
                .is_err()
        );
        drop(one);
        tokio::time::timeout(std::time::Duration::from_secs(1), two.acquire(&key))
            .await
            .unwrap()
            .unwrap();
        two.release(&key);
    }
}
