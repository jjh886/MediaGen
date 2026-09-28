use std::time::Duration;

use actix_web::web;
use chrono::Utc;

use crate::store::{AccountRecord, WeixinPreferencesRecord};
use crate::store_weixin::WeixinDesktopTaskEvent;
use crate::ServerState;

const SCAN_DELAY: Duration = Duration::from_secs(5);
const TASK_CURSOR_KEY: &str = "desktop_tasks";
const NO_ACCOUNT_STATE_KEY: &str = "__no_accounts__";

pub(crate) async fn run_notification_worker(state: web::Data<ServerState>) {
    loop {
        if let Err(error) = scan_notifications(&state) {
            eprintln!("[mediagen-server] 扫描微信主动通知失败: {error}");
        }
        tokio::select! {
            _ = state.weixin.wake.notified() => {}
            _ = tokio::time::sleep(SCAN_DELAY) => {}
        }
    }
}

fn scan_notifications(state: &ServerState) -> Result<(), String> {
    let now = Utc::now();
    let binding_active = state
        .store
        .get_weixin_binding()
        .is_some_and(|binding| binding.status == "active");
    let preferences = state.store.get_weixin_preferences();
    let cursor = state
        .store
        .get_weixin_notification_cursor(TASK_CURSOR_KEY)
        .and_then(|value| crate::store_weixin::parse_time(&value));
    let task_events = cursor
        .map(|cursor| state.store.list_desktop_task_events(cursor, now))
        .unwrap_or_default();
    state
        .store
        .set_weixin_notification_cursor(TASK_CURSOR_KEY, now);
    let proactive = binding_active && preferences.proactive_notifications_enabled;
    if proactive {
        for event in task_events.into_iter().filter(|event| {
            should_notify_task(
                &event.status,
                preferences.notify_desktop_tasks,
                preferences.notify_task_successes,
            )
        }) {
            queue_desktop_task(state, event)?;
        }
    }
    update_account_notifications(
        state,
        proactive && preferences.notify_no_accounts,
        proactive && preferences.notify_account_actions,
        &preferences,
    )?;
    Ok(())
}

fn queue_desktop_task(state: &ServerState, event: WeixinDesktopTaskEvent) -> Result<(), String> {
    let status = match event.status.as_str() {
        "completed" => "完成",
        "failed" => "失败",
        _ => return Ok(()),
    };
    let mut text = format!(
        "任务{status}：{} {}",
        short_id(&event.task_id),
        truncate(&event.prompt, 40)
    );
    if event.status == "failed" {
        if let Some(reason) = event.error_message.filter(|value| !value.is_empty()) {
            text.push_str(&format!("\n原因：{}", truncate(&reason, 100)));
        }
    }
    queue_notification(
        state,
        &format!("desktop-task-{}-{}", event.task_id, event.status),
        &text,
    )
}

fn update_account_notifications(
    state: &ServerState,
    no_account_enabled: bool,
    action_enabled: bool,
    _preferences: &WeixinPreferencesRecord,
) -> Result<(), String> {
    let accounts = state.store.list_accounts(None);
    update_no_account_notification(state, &accounts, no_account_enabled)?;
    for account in accounts {
        update_account_notification(state, &account, action_enabled)?;
    }
    Ok(())
}

fn update_no_account_notification(
    state: &ServerState,
    accounts: &[AccountRecord],
    enabled: bool,
) -> Result<(), String> {
    let previous_alert_sent = state
        .store
        .get_weixin_account_notice_state(NO_ACCOUNT_STATE_KEY)
        .is_some_and(|(_, sent)| sent);
    let states = accounts
        .iter()
        .map(|account| account.login_state.as_str())
        .collect::<Vec<_>>();
    let missing = should_notify_no_available_account(&states);
    if enabled && missing && !previous_alert_sent {
        queue_notification(
            state,
            &format!("account-none-action-required-{}", uuid::Uuid::new_v4()),
            "当前没有可用生成账号，任务会继续等待。请在控制台账号页面查看。",
        )?;
    }
    state.store.save_weixin_account_notice_state(
        NO_ACCOUNT_STATE_KEY,
        if missing { "missing" } else { "available" },
        enabled && missing,
    );
    Ok(())
}

fn update_account_notification(
    state: &ServerState,
    account: &AccountRecord,
    enabled: bool,
) -> Result<(), String> {
    let current = login_state_key(&account.login_state);
    let previous = state.store.get_weixin_account_notice_state(&account.id);
    let previous_alert_sent = previous.as_ref().is_some_and(|(_, sent)| *sent);
    let changed = previous
        .as_ref()
        .is_none_or(|(previous_state, _)| previous_state != current);
    let alert_sent = resolve_account_alert(state, account, enabled, changed, previous_alert_sent)?;
    state
        .store
        .save_weixin_account_notice_state(&account.id, current, alert_sent);
    Ok(())
}

fn resolve_account_alert(
    state: &ServerState,
    account: &AccountRecord,
    enabled: bool,
    changed: bool,
    previous_alert_sent: bool,
) -> Result<bool, String> {
    let actionable = is_actionable(&account.login_state);
    if should_queue_account_action(enabled, actionable, changed, previous_alert_sent) {
        queue_account_action_required(state, account)?;
        return Ok(true);
    }
    if should_queue_account_recovery(enabled, account, changed, previous_alert_sent) {
        queue_account_recovered(state, account)?;
        return Ok(false);
    }
    if should_clear_account_alert(enabled, actionable, account) {
        return Ok(false);
    }
    Ok(previous_alert_sent)
}

fn should_queue_account_action(
    enabled: bool,
    actionable: bool,
    _changed: bool,
    previous_alert_sent: bool,
) -> bool {
    enabled && actionable && !previous_alert_sent
}

fn should_queue_account_recovery(
    enabled: bool,
    account: &AccountRecord,
    changed: bool,
    previous_alert_sent: bool,
) -> bool {
    enabled && account.login_state == "available" && changed && previous_alert_sent
}

fn should_clear_account_alert(enabled: bool, actionable: bool, account: &AccountRecord) -> bool {
    !enabled || (!actionable && account.login_state != "available")
}

fn queue_account_action_required(
    state: &ServerState,
    account: &AccountRecord,
) -> Result<(), String> {
    queue_notification(
        state,
        &format!(
            "account-{}-{}-{}",
            account.id,
            login_state_key(&account.login_state),
            uuid::Uuid::new_v4()
        ),
        &format!(
            "账号需要处理\n账号：{}\n状态：{}",
            account_name(account),
            login_state_label(&account.login_state)
        ),
    )
}

fn queue_account_recovered(state: &ServerState, account: &AccountRecord) -> Result<(), String> {
    queue_notification(
        state,
        &format!("account-{}-recovered-{}", account.id, uuid::Uuid::new_v4()),
        &format!("账号已恢复可用\n账号：{}", account_name(account)),
    )
}

fn queue_notification(state: &ServerState, client_id: &str, text: &str) -> Result<(), String> {
    state.store.enqueue_weixin_outbound(
        client_id,
        "text",
        &serde_json::json!({ "text": text }).to_string(),
    );
    state.weixin.wake.notify_waiters();
    Ok(())
}

fn should_notify_task(status: &str, failures: bool, successes: bool) -> bool {
    (status == "failed" && failures) || (status == "completed" && successes)
}

fn should_notify_no_available_account(states: &[&str]) -> bool {
    !states.contains(&"available") && !states.iter().copied().any(is_actionable)
}

fn is_actionable(state: &str) -> bool {
    matches!(
        state,
        "login_expired" | "verification_required" | "total_quota_exhausted"
    )
}

fn login_state_key(state: &str) -> &'static str {
    match state {
        "available" => "available",
        "daily_quota_exhausted" => "daily_quota_exhausted",
        "total_quota_exhausted" => "total_quota_exhausted",
        "login_expired" => "login_expired",
        "verification_required" => "verification_required",
        "cooldown" => "cooldown",
        "disabled" => "disabled",
        _ => "unknown",
    }
}

fn login_state_label(state: &str) -> &'static str {
    match login_state_key(state) {
        "login_expired" => "登录过期",
        "verification_required" => "需要人工验证",
        "total_quota_exhausted" => "总额度耗尽",
        "daily_quota_exhausted" => "今日额度耗尽",
        "cooldown" => "冷却中",
        "disabled" => "已停用",
        "available" => "可用",
        _ => "未知",
    }
}

fn account_name(account: &AccountRecord) -> String {
    let value = if account.name.trim().is_empty() {
        &account.id
    } else {
        &account.name
    };
    format!("{} / {}", account.platform, truncate(value, 32))
}

fn short_id(value: &str) -> &str {
    value.get(..8).unwrap_or(value)
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn account_notifications_deduplicate_until_recovery_and_survive_reload() {
        let root = std::env::temp_dir().join(format!("notice-test-{}", uuid::Uuid::new_v4()));
        let state = crate::test_fixtures::server_state(
            root.clone(),
            mediagen_plugin_runtime::PluginLoader::empty(),
        );
        let mut account =
            crate::test_fixtures::account_record("account", "provider", "测试账号", "key");
        account.login_state = "verification_required".into();
        super::update_account_notification(&state, &account, true).unwrap();
        super::update_account_notification(&state, &account, true).unwrap();
        assert_eq!(state.store.weixin_messages.lock().unwrap().len(), 1);
        let state = crate::test_fixtures::server_state(
            root,
            mediagen_plugin_runtime::PluginLoader::empty(),
        );
        super::update_account_notification(&state, &account, true).unwrap();
        assert_eq!(state.store.weixin_messages.lock().unwrap().len(), 1);
        account.login_state = "login_expired".into();
        super::update_account_notification(&state, &account, true).unwrap();
        assert_eq!(state.store.weixin_messages.lock().unwrap().len(), 1);
        account.login_state = "available".into();
        super::update_account_notification(&state, &account, true).unwrap();
        account.login_state = "verification_required".into();
        super::update_account_notification(&state, &account, true).unwrap();
        let messages = state.store.weixin_messages.lock().unwrap();
        assert_eq!(messages.len(), 3);
        assert_ne!(messages[0].client_id, messages[2].client_id);
    }

    #[test]
    fn task_notification_categories_are_independent() {
        assert!(super::should_notify_task("failed", true, false));
        assert!(!super::should_notify_task("completed", true, false));
        assert!(super::should_notify_task("completed", false, true));
    }

    #[test]
    fn no_account_notice_skips_manual_takeover_states() {
        assert!(super::should_notify_no_available_account(&[
            "cooldown",
            "daily_quota_exhausted"
        ]));
        assert!(!super::should_notify_no_available_account(&["available"]));
        assert!(!super::should_notify_no_available_account(&[
            "login_expired"
        ]));
    }
}
