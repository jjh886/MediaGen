use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::store::{
    AppStore, WeixinAccountNotificationStateRecord, WeixinBindingRecord, WeixinMessageRecord,
    WeixinNotificationCursorRecord, WeixinPreferencesRecord, WeixinTaskLinkRecord,
};

pub(crate) const WEIXIN_BINDING_ID: &str = "default";

#[derive(Clone, Debug)]
pub(crate) struct WeixinTaskDelivery {
    pub task_id: String,
    pub status: String,
    pub result_url: Option<String>,
    pub result_data: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct WeixinDesktopTaskEvent {
    pub task_id: String,
    pub status: String,
    pub prompt: String,
    pub error_message: Option<String>,
}

impl AppStore {
    pub(crate) fn get_weixin_binding(&self) -> Option<WeixinBindingRecord> {
        self.weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub(crate) fn save_weixin_binding(&self, binding: WeixinBindingRecord) {
        *self
            .weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(binding);
        self.save();
    }

    pub(crate) fn remove_weixin_binding(&self) {
        *self
            .weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = None;
        self.weixin_messages
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.weixin_task_links
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.weixin_notification_cursors
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.weixin_account_notification_states
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.save();
    }

    pub(crate) fn renew_weixin_activation(
        &self,
        code_hash: &str,
        expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let mut binding_guard = self
            .weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(binding) = binding_guard.as_mut() else {
            return Err("微信尚未绑定".to_string());
        };
        binding.status = "pending_activation".to_string();
        binding.activation_code_hash = code_hash.to_string();
        binding.activation_expires_at = Some(expires_at.to_rfc3339());
        binding.updated_at = now.to_rfc3339();
        drop(binding_guard);
        self.save();
        Ok(())
    }

    pub(crate) fn activate_weixin_binding(&self, context_ciphertext: &str) {
        let now = Utc::now().to_rfc3339();
        if let Some(binding) = self
            .weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_mut()
        {
            binding.status = "active".to_string();
            binding.context_token_ciphertext = context_ciphertext.to_string();
            binding.activation_code_hash.clear();
            binding.activation_expires_at = None;
            binding.updated_at = now;
        }
        self.save();
    }

    pub(crate) fn update_weixin_context(&self, context_ciphertext: &str) {
        if let Some(binding) = self
            .weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_mut()
        {
            binding.context_token_ciphertext = context_ciphertext.to_string();
            binding.updated_at = Utc::now().to_rfc3339();
        }
        self.save();
    }

    pub(crate) fn mark_weixin_reauth_required(&self, reason: &str) {
        if let Some(binding) = self
            .weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_mut()
        {
            binding.status = "reauth_required".to_string();
            binding.updated_at = Utc::now().to_rfc3339();
        }
        self.add_audit_log(
            "weixin",
            "warn",
            "微信绑定需要重新认证".to_string(),
            reason.to_string(),
            None,
        );
    }

    pub(crate) fn touch_weixin_inbound(&self) {
        self.touch_weixin_binding(|binding, now| binding.last_inbound_at = Some(now));
    }

    pub(crate) fn touch_weixin_outbound(&self) {
        self.touch_weixin_binding(|binding, now| binding.last_outbound_at = Some(now));
    }

    fn touch_weixin_binding(&self, update: impl FnOnce(&mut WeixinBindingRecord, String)) {
        let now = Utc::now().to_rfc3339();
        if let Some(binding) = self
            .weixin_binding
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_mut()
        {
            update(binding, now.clone());
            binding.updated_at = now;
        }
        self.save();
    }

    pub(crate) fn get_weixin_preferences(&self) -> WeixinPreferencesRecord {
        self.weixin_preferences
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub(crate) fn save_weixin_preferences(&self, mut preferences: WeixinPreferencesRecord) {
        preferences.binding_id = WEIXIN_BINDING_ID.to_string();
        preferences.default_max_images = preferences.default_max_images.clamp(1, 4);
        preferences.updated_at = Utc::now().to_rfc3339();
        *self
            .weixin_preferences
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = preferences;
        self.save();
    }

    pub(crate) fn enqueue_weixin_outbound(
        &self,
        client_id: &str,
        message_type: &str,
        content_json: &str,
    ) {
        self.enqueue_weixin_message("outbound", None, client_id, message_type, content_json);
    }

    pub(crate) fn save_weixin_inbound_batch(&self, messages: &[(String, String)], cursor: &str) {
        for (remote_id, content_json) in messages {
            self.enqueue_weixin_message(
                "inbound",
                Some(remote_id.clone()),
                &format!("inbound-{remote_id}"),
                "command",
                content_json,
            );
        }
        if !cursor.is_empty() {
            if let Some(binding) = self
                .weixin_binding
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .as_mut()
            {
                binding.get_updates_buf = cursor.to_string();
                binding.updated_at = Utc::now().to_rfc3339();
            }
        }
        self.save();
    }

    fn enqueue_weixin_message(
        &self,
        direction: &str,
        remote_message_id: Option<String>,
        client_id: &str,
        message_type: &str,
        content_json: &str,
    ) {
        let mut messages = self
            .weixin_messages
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let duplicate = messages.iter().any(|item| {
            item.direction == direction
                && ((remote_message_id.is_some() && item.remote_message_id == remote_message_id)
                    || (!client_id.is_empty() && item.client_id == client_id))
        });
        if duplicate {
            return;
        }
        let now = Utc::now().to_rfc3339();
        messages.push(WeixinMessageRecord {
            id: format!("weixin-msg-{}", Uuid::new_v4()),
            binding_id: WEIXIN_BINDING_ID.to_string(),
            direction: direction.to_string(),
            remote_message_id,
            client_id: client_id.to_string(),
            message_type: message_type.to_string(),
            content_json: content_json.to_string(),
            status: "pending".to_string(),
            attempt_count: 0,
            next_attempt_at: now.clone(),
            last_error: None,
            created_at: now,
            processed_at: None,
        });
    }

    pub(crate) fn next_weixin_message(
        &self,
        direction: &str,
        now: DateTime<Utc>,
    ) -> Option<WeixinMessageRecord> {
        let mut messages = self
            .weixin_messages
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let item = messages
            .iter_mut()
            .filter(|item| item.direction == direction && item.status == "pending")
            .filter(|item| parse_time(&item.next_attempt_at).is_some_and(|due| due <= now))
            .min_by(|left, right| left.created_at.cmp(&right.created_at))?;
        item.status = "processing".to_string();
        item.attempt_count = item.attempt_count.saturating_add(1);
        item.processed_at = Some(now.to_rfc3339());
        let value = item.clone();
        drop(messages);
        self.save();
        Some(value)
    }

    pub(crate) fn retry_weixin_message(
        &self,
        id: &str,
        next_attempt_at: DateTime<Utc>,
        error: &str,
    ) {
        if let Some(message) = self
            .weixin_messages
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter_mut()
            .find(|item| item.id == id)
        {
            message.status = "pending".to_string();
            message.next_attempt_at = next_attempt_at.to_rfc3339();
            message.last_error = Some(error.chars().take(400).collect());
        }
        self.save();
    }

    pub(crate) fn finish_weixin_message(&self, id: &str, status: &str) {
        if let Some(message) = self
            .weixin_messages
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter_mut()
            .find(|item| item.id == id)
        {
            message.status = status.to_string();
            message.processed_at = Some(Utc::now().to_rfc3339());
        }
        self.save();
    }

    pub(crate) fn count_recent_weixin_commands(&self, since: DateTime<Utc>) -> usize {
        self.weixin_messages
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .filter(|item| item.direction == "inbound")
            .filter(|item| parse_time(&item.created_at).is_some_and(|value| value >= since))
            .count()
    }

    pub(crate) fn link_weixin_task(&self, inbound_message_id: &str, task_id: &str) {
        let now = Utc::now().to_rfc3339();
        let mut links = self
            .weixin_task_links
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if links.iter().any(|item| item.task_id == task_id) {
            return;
        }
        links.push(WeixinTaskLinkRecord {
            binding_id: WEIXIN_BINDING_ID.to_string(),
            inbound_message_id: inbound_message_id.to_string(),
            task_id: task_id.to_string(),
            result_delivery_status: "pending".to_string(),
            created_at: now.clone(),
            updated_at: now,
        });
        drop(links);
        self.save();
    }

    pub(crate) fn next_weixin_task_delivery(&self) -> Option<WeixinTaskDelivery> {
        let tasks = self.tasks.lock().unwrap_or_else(|err| err.into_inner());
        let links = self
            .weixin_task_links
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        links
            .iter()
            .filter(|link| link.result_delivery_status == "pending")
            .filter_map(|link| {
                let task = tasks.iter().find(|task| task.id == link.task_id)?;
                matches!(task.status.as_str(), "completed" | "failed" | "cancelled").then(|| {
                    WeixinTaskDelivery {
                        task_id: task.id.clone(),
                        status: task.status.clone(),
                        result_url: task.result_url.clone(),
                        result_data: task.result_data.clone(),
                        error_message: task.error_message.clone(),
                    }
                })
            })
            .next()
    }

    pub(crate) fn finish_weixin_task_delivery(&self, task_id: &str, status: &str) {
        if let Some(link) = self
            .weixin_task_links
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter_mut()
            .find(|item| item.task_id == task_id)
        {
            link.result_delivery_status = status.to_string();
            link.updated_at = Utc::now().to_rfc3339();
        }
        self.save();
    }

    pub(crate) fn get_weixin_notification_cursor(&self, key: &str) -> Option<String> {
        self.weixin_notification_cursors
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.value.clone())
    }

    pub(crate) fn set_weixin_notification_cursor(&self, key: &str, value: DateTime<Utc>) {
        let mut cursors = self
            .weixin_notification_cursors
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let now = Utc::now().to_rfc3339();
        if let Some(cursor) = cursors.iter_mut().find(|item| item.key == key) {
            cursor.value = value.to_rfc3339();
            cursor.updated_at = now;
        } else {
            cursors.push(WeixinNotificationCursorRecord {
                key: key.to_string(),
                value: value.to_rfc3339(),
                updated_at: now,
            });
        }
        drop(cursors);
        self.save();
    }

    pub(crate) fn list_desktop_task_events(
        &self,
        cursor: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Vec<WeixinDesktopTaskEvent> {
        self.tasks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .filter(|task| matches!(task.status.as_str(), "completed" | "failed"))
            .filter(|task| {
                task.completed_at
                    .as_deref()
                    .and_then(parse_time)
                    .is_some_and(|at| at > cursor && at <= now)
            })
            .map(|task| WeixinDesktopTaskEvent {
                task_id: task.id.clone(),
                status: task.status.clone(),
                prompt: task.prompt.clone(),
                error_message: task.error_message.clone(),
            })
            .collect()
    }

    pub(crate) fn get_weixin_account_notice_state(
        &self,
        account_id: &str,
    ) -> Option<(String, bool)> {
        self.weixin_account_notification_states
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .find(|item| item.account_id == account_id)
            .map(|item| (item.last_observed_state.clone(), item.alert_sent))
    }

    pub(crate) fn save_weixin_account_notice_state(
        &self,
        account_id: &str,
        observed_state: &str,
        alert_sent: bool,
    ) {
        let mut states = self
            .weixin_account_notification_states
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let now = Utc::now().to_rfc3339();
        if let Some(state) = states.iter_mut().find(|item| item.account_id == account_id) {
            state.last_observed_state = observed_state.to_string();
            state.alert_sent = alert_sent;
            state.updated_at = now;
        } else {
            states.push(WeixinAccountNotificationStateRecord {
                account_id: account_id.to_string(),
                last_observed_state: observed_state.to_string(),
                alert_sent,
                updated_at: now,
            });
        }
        drop(states);
        self.save();
    }
}

pub(crate) fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}
