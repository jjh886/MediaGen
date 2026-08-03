use actix_web::{web, HttpResponse};
use mediagen_core::TaskKind;
use serde_json::json;

use super::types::{
    BindingActionResult, WeixinActionRequest, WeixinModelView, WeixinSettingsUpdate,
    WeixinSettingsView,
};
use crate::store::WeixinPreferencesRecord;
use crate::ServerState;

pub(crate) fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/weixin/settings", web::get().to(get_settings))
        .route("/api/weixin/binding", web::post().to(start_binding))
        .route("/api/weixin/actions", web::post().to(action));
}

async fn get_settings(state: web::Data<ServerState>) -> HttpResponse {
    let binding = state.store.get_weixin_binding();
    let preferences = state.store.get_weixin_preferences();
    HttpResponse::Ok().json(json!({
        "data": WeixinSettingsView {
            status: binding
                .as_ref()
                .map(|value| value.status.clone())
                .unwrap_or_else(|| "unbound".to_string()),
            masked_user_id: binding
                .as_ref()
                .map(|value| mask_user_id(&value.ilink_user_id))
                .unwrap_or_default(),
            last_inbound_at: binding.as_ref().and_then(|value| value.last_inbound_at.clone()),
            last_outbound_at: binding.as_ref().and_then(|value| value.last_outbound_at.clone()),
            generation_enabled: preferences.generation_enabled,
            direct_prompt_enabled: preferences.direct_prompt_enabled,
            default_model_id: preferences.default_model_id,
            default_aspect_ratio: preferences.default_aspect_ratio,
            default_max_images: preferences.default_max_images,
            proactive_notifications_enabled: preferences.proactive_notifications_enabled,
            notify_desktop_tasks: preferences.notify_desktop_tasks,
            notify_task_successes: preferences.notify_task_successes,
            notify_no_accounts: preferences.notify_no_accounts,
            notify_account_actions: preferences.notify_account_actions,
            quiet_hours_json: preferences.quiet_hours_json,
            models: image_models(&state),
        }
    }))
}

async fn start_binding(state: web::Data<ServerState>) -> HttpResponse {
    json_result(state.weixin.start_binding(&state).await)
}

async fn action(
    state: web::Data<ServerState>,
    body: web::Json<WeixinActionRequest>,
) -> HttpResponse {
    json_result(run_action(&state, body.into_inner()).await)
}

async fn run_action(
    state: &ServerState,
    request: WeixinActionRequest,
) -> Result<BindingActionResult, String> {
    let result = match request.action.as_str() {
        "poll_binding" => {
            let session_id = required_session(&request)?;
            state.weixin.poll_binding(state, session_id).await
        }
        "submit_verify_code" => {
            let session_id = required_session(&request)?;
            let value = request.value.as_deref().unwrap_or_default();
            state.weixin.submit_verify_code(session_id, value).await
        }
        "cancel_binding" => {
            let session_id = required_session(&request)?;
            Ok(state.weixin.cancel_binding(session_id).await)
        }
        "send_test" => send_test(state),
        "save_settings" => save_settings(state, request.settings),
        "remove_binding" => remove_binding(state).await,
        _ => Err("不支持的微信设置操作".to_string()),
    };
    result
}

fn send_test(state: &ServerState) -> Result<BindingActionResult, String> {
    ensure_active_binding(state)?;
    super::router::enqueue_text(
        state,
        &format!("test-{}", chrono::Utc::now().timestamp_millis()),
        "MediaGen 微信连接正常。",
    )?;
    Ok(result("queued", "测试消息已加入发送队列"))
}

fn save_settings(
    state: &ServerState,
    update: Option<WeixinSettingsUpdate>,
) -> Result<BindingActionResult, String> {
    let update = update.ok_or_else(|| "微信设置不能为空".to_string())?;
    validate_settings_update(state, &update)?;
    let mut preferences = state.store.get_weixin_preferences();
    apply_settings_update(&mut preferences, update);
    state.store.save_weixin_preferences(preferences);
    state.weixin.wake.notify_waiters();
    Ok(result("saved", "微信设置已保存"))
}

async fn remove_binding(state: &ServerState) -> Result<BindingActionResult, String> {
    state.weixin.clear_sessions().await;
    state.store.remove_weixin_binding();
    state.weixin.wake.notify_waiters();
    Ok(result("unbound", "已移除本机微信绑定"))
}

fn validate_settings_update(
    state: &ServerState,
    update: &WeixinSettingsUpdate,
) -> Result<(), String> {
    if update.default_max_images == 0 || update.default_max_images > 4 {
        return Err("默认生成张数只能是 1 到 4".to_string());
    }
    if !matches!(
        update.default_aspect_ratio.as_str(),
        "auto" | "1:1" | "9:16" | "16:9" | "2:1"
    ) {
        return Err("默认比例无效".to_string());
    }
    let model_id = update.default_model_id.trim();
    if update.generation_enabled && model_id.is_empty() {
        return Err("开启微信生图前请先选择默认模型".to_string());
    }
    let model_exists = state
        .plugin_loader
        .all_models()
        .into_iter()
        .any(|(_, model)| model.task_kind == TaskKind::Image && model.model_id == model_id);
    if !model_id.is_empty() && !model_exists {
        return Err("默认模型不存在或不支持图片生成".to_string());
    }
    validate_quiet_hours(&update.quiet_hours_json)
}

fn validate_quiet_hours(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let parsed: serde_json::Value =
        serde_json::from_str(value).map_err(|_| "安静时段必须是有效 JSON".to_string())?;
    if !parsed.is_object() {
        return Err("安静时段必须是对象格式".to_string());
    }
    Ok(())
}

fn apply_settings_update(preferences: &mut WeixinPreferencesRecord, update: WeixinSettingsUpdate) {
    preferences.generation_enabled = update.generation_enabled;
    preferences.direct_prompt_enabled = update.direct_prompt_enabled;
    preferences.default_model_id = update.default_model_id;
    preferences.default_aspect_ratio = update.default_aspect_ratio;
    preferences.default_max_images = update.default_max_images;
    preferences.proactive_notifications_enabled = update.proactive_notifications_enabled;
    preferences.notify_weixin_tasks = true;
    preferences.notify_desktop_tasks = update.notify_desktop_tasks;
    preferences.notify_task_successes = update.notify_task_successes;
    preferences.notify_no_accounts = update.notify_no_accounts;
    preferences.notify_account_actions = update.notify_account_actions;
    preferences.quiet_hours_json = update.quiet_hours_json;
}

fn ensure_active_binding(state: &ServerState) -> Result<(), String> {
    state
        .store
        .get_weixin_binding()
        .filter(|binding| binding.status == "active")
        .map(|_| ())
        .ok_or_else(|| "微信尚未完成绑定".to_string())
}

fn image_models(state: &ServerState) -> Vec<WeixinModelView> {
    state
        .plugin_loader
        .all_models()
        .into_iter()
        .filter(|(_, model)| model.task_kind == TaskKind::Image)
        .map(|(manifest, model)| WeixinModelView {
            model_id: model.model_id.clone(),
            display_name: model.display_name.clone(),
            platform: manifest.platform_id.clone(),
        })
        .collect()
}

fn required_session(request: &WeixinActionRequest) -> Result<&str, String> {
    request
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "扫码会话 ID 缺失".to_string())
}

fn mask_user_id(value: &str) -> String {
    if value.chars().count() <= 8 {
        return "***".to_string();
    }
    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{prefix}***{suffix}")
}

fn result(status: &str, message: &str) -> BindingActionResult {
    BindingActionResult {
        status: status.to_string(),
        message: message.to_string(),
        session_id: None,
        qr_image: None,
        activation_code: None,
    }
}

fn json_result(result: Result<BindingActionResult, String>) -> HttpResponse {
    match result {
        Ok(value) => HttpResponse::Ok().json(json!({ "data": value })),
        Err(error) => HttpResponse::BadRequest().json(json!({ "error": error })),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn masks_weixin_user_id() {
        assert_eq!(super::mask_user_id("abcdefghijk"), "abcd***hijk");
        assert_eq!(super::mask_user_id("short"), "***");
    }
}
