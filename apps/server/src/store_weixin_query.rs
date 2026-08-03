use chrono::{DateTime, Utc};

use crate::store::{AppStore, TaskRecord, WeixinTaskLinkRecord};

impl AppStore {
    pub(crate) fn pending_weixin_task_link(&self, task_id: &str) -> Option<WeixinTaskLinkRecord> {
        self.weixin_task_links
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .find(|link| link.task_id == task_id && link.result_delivery_status == "pending")
            .cloned()
    }

    pub(crate) fn count_recent_weixin_tasks(&self, since: DateTime<Utc>) -> usize {
        self.weixin_task_links
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .filter(|link| {
                crate::store_weixin::parse_time(&link.created_at)
                    .is_some_and(|created_at| created_at >= since)
            })
            .count()
    }

    /// 微信里输入完整任务 ID 比较麻烦，所以这里允许输入短前缀，但必须唯一。
    pub(crate) fn find_task_by_id_or_prefix(
        &self,
        value: &str,
    ) -> Result<Option<TaskRecord>, String> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(None);
        }
        let tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(task) = tasks.iter().find(|task| task.id == value) {
            return Ok(Some(task.clone()));
        }
        let matches = tasks
            .iter()
            .filter(|task| task.id.starts_with(value))
            .take(2)
            .cloned()
            .collect::<Vec<_>>();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.into_iter().next()),
            _ => Err("找到多个匹配任务，请输入更完整的任务 ID".to_string()),
        }
    }
}
