use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::settings::AppSettings;

#[derive(Default)]
pub struct ImageTaskQueue {
    state: Mutex<QueueState>,
    notify: Notify,
}

#[derive(Default)]
struct QueueState {
    active: u32,
    queued: u32,
}

pub struct ImageTaskReservation {
    queue: Arc<ImageTaskQueue>,
    activated: bool,
}

pub struct ImageTaskPermit {
    queue: Arc<ImageTaskQueue>,
}

impl ImageTaskQueue {
    pub fn reserve(
        self: &Arc<Self>,
        settings: &AppSettings,
    ) -> Result<ImageTaskReservation, String> {
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        let limit = settings.image_task_queue_limit;
        if limit > 0 && state.active + state.queued >= limit {
            return Err("图片任务队列已满，请稍后重试。".to_string());
        }
        state.queued = state.queued.saturating_add(1);
        Ok(ImageTaskReservation {
            queue: self.clone(),
            activated: false,
        })
    }

    pub fn direct_waiter_count(&self) -> u32 {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .queued
    }

    /// 后台队列只使用空闲并发槽；有同步请求等待时，优先让同步请求执行。
    pub fn try_acquire_background(
        self: &Arc<Self>,
        settings: &AppSettings,
    ) -> Option<ImageTaskPermit> {
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if state.queued > 0 || state.active >= settings.image_task_concurrency.max(1) {
            return None;
        }
        state.active = state.active.saturating_add(1);
        Some(ImageTaskPermit {
            queue: self.clone(),
        })
    }
}

impl ImageTaskReservation {
    pub async fn activate(mut self, settings: &AppSettings) -> ImageTaskPermit {
        let concurrency = settings.image_task_concurrency.max(1);
        loop {
            if self.try_activate(concurrency) {
                return ImageTaskPermit {
                    queue: self.queue.clone(),
                };
            }
            let queue = self.queue.clone();
            let notified = queue.notify.notified();
            if self.try_activate(concurrency) {
                return ImageTaskPermit {
                    queue: self.queue.clone(),
                };
            }
            notified.await;
        }
    }

    fn try_activate(&mut self, concurrency: u32) -> bool {
        let mut state = self
            .queue
            .state
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if state.active >= concurrency {
            return false;
        }
        state.queued = state.queued.saturating_sub(1);
        state.active = state.active.saturating_add(1);
        self.activated = true;
        true
    }
}

impl Drop for ImageTaskReservation {
    fn drop(&mut self) {
        if self.activated {
            return;
        }
        let mut state = self
            .queue
            .state
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        state.queued = state.queued.saturating_sub(1);
        self.queue.notify.notify_one();
    }
}

impl Drop for ImageTaskPermit {
    fn drop(&mut self) {
        let mut state = self
            .queue
            .state
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        state.active = state.active.saturating_sub(1);
        self.queue.notify.notify_one();
    }
}
