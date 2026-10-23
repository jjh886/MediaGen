use chrono::Utc;
use uuid::Uuid;

use crate::store::{AppStore, TaskEventRecord, TaskRecord};
use crate::store_helpers::first_result_url;

impl AppStore {
    pub fn list_tasks(&self, platform: Option<&str>) -> Vec<TaskRecord> {
        let mut tasks: Vec<_> = self
            .tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .filter(|task| platform.is_none_or(|value| value.is_empty() || task.platform == value))
            .cloned()
            .collect();
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        tasks
    }

    pub fn task_by_id(&self, id: &str) -> Option<TaskRecord> {
        self.tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .find(|task| task.id == id)
            .cloned()
    }

    pub fn insert_task(&self, task: TaskRecord) {
        if task.status == "processing" {
            if let Some(id) = &task.account_id {
                self.mark_account_dispatched(id);
            }
        }
        self.tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(task);
        self.save();
    }

    /// 同步协议排队后在原任务上开始执行，不能插入重复占用记录。
    pub(crate) fn dispatch_protocol_task(&self, task: TaskRecord) -> bool {
        {
            let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
            if let Some(existing) = tasks.iter_mut().find(|existing| existing.id == task.id) {
                if !matches!(
                    existing.status.as_str(),
                    "pending" | "queued" | "failed" | "completed"
                ) {
                    return false;
                }
                *existing = task;
            } else {
                tasks.push(task);
            }
        }
        self.save();
        true
    }

    pub fn add_task_event(&self, task_id: &str, level: &str, message: String) {
        self.task_events
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(TaskEventRecord {
                id: format!("event-{}", Uuid::new_v4()),
                task_id: task_id.to_string(),
                level: level.to_string(),
                message,
                created_at: Utc::now().to_rfc3339(),
            });
        self.save();
    }

    pub fn complete_task(&self, id: &str, result: serde_json::Value) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            if task.status == "cancelled" {
                return;
            }
            task.status = "completed".to_string();
            task.result_data = Some(result.to_string());
            task.completed_at = Some(Utc::now().to_rfc3339());
            if let Some(url) = first_result_url(&result) {
                task.result_url = Some(url);
            }
        })
    }

    pub(crate) fn clear_task_progress(&self, id: &str) {
        self.update_task(id, |task| task.result_data = None);
    }

    pub(crate) fn save_task_progress(
        &self,
        id: &str,
        progress: serde_json::Value,
    ) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            if task.status == "processing" {
                task.result_data = Some(progress.to_string());
            }
        })
    }

    pub(crate) fn set_task_prompt(&self, id: &str, prompt: String) {
        self.update_task(id, |task| task.prompt = prompt);
    }

    pub fn set_task_account(&self, id: &str, account_id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.account_id = Some(account_id.to_string());
        })
    }

    pub fn start_task(&self, id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "processing".to_string();
            task.error_message = None;
            task.completed_at = None;
        })
    }

    pub fn fail_task(&self, id: &str, message: String) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            if task.status == "cancelled" {
                return;
            }
            task.status = "failed".to_string();
            task.error_message = Some(message);
            task.completed_at = Some(Utc::now().to_rfc3339());
        })
    }

    pub fn require_task_verification(&self, id: &str, message: String) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "verification_required".to_string();
            task.error_message = Some(message);
        })
    }

    pub fn resume_task_after_verification(&self, id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "queued".to_string();
            task.error_message = None;
            task.completed_at = None;
        })
    }

    pub fn cancel_task(&self, id: &str) -> Option<TaskRecord> {
        self.update_task(id, |task| {
            task.status = "cancelled".to_string();
            task.error_message = Some("用户取消".to_string());
            task.completed_at = Some(Utc::now().to_rfc3339());
        })
    }

    fn update_task(&self, id: &str, update: impl FnOnce(&mut TaskRecord)) -> Option<TaskRecord> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let task = tasks.iter_mut().find(|task| task.id == id)?;
        update(task);
        let task = task.clone();
        drop(tasks);
        self.save();
        self.dispatch_notify.notify_waiters();
        Some(task)
    }

    pub fn delete_task(&self, id: &str) -> bool {
        let mut tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let before = tasks.len();
        tasks.retain(|task| task.id != id);
        let changed = tasks.len() != before;
        drop(tasks);
        if changed {
            self.save();
        }
        changed
    }
}
