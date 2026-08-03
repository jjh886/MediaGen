use std::collections::BTreeMap;

use actix_web::web;
use chrono::{Duration as ChronoDuration, Utc};
use mediagen_core::TaskKind;
use serde::Deserialize;
use serde_json::{json, Value};

use super::types::WeixinInboundImage;
use crate::image_request::{ImageEndpoint, OpenAiImageRef, OpenAiImageRequest};
use crate::store::WeixinMessageRecord;
use crate::ServerState;

const MAX_PROMPT_CHARS: usize = 4_000;
const MAX_COMMANDS_PER_MINUTE: usize = 10;
const MAX_IMAGE_TASKS_PER_MINUTE: usize = 3;
const WEIXIN_OWNER_TOKEN: &str = "__mediagen_weixin_internal__";

#[derive(Deserialize)]
struct InboundEnvelope {
    from_user_id: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    image: Option<WeixinInboundImage>,
}

pub(crate) async fn process_message(
    state: web::Data<ServerState>,
    message: WeixinMessageRecord,
) -> Result<(), String> {
    let envelope: InboundEnvelope = serde_json::from_str(&message.content_json)
        .map_err(|error| format!("微信指令数据无效: {error}"))?;
    let text = envelope
        .text
        .as_deref()
        .map(|value| sanitize_text(value, MAX_PROMPT_CHARS))
        .filter(|value| !value.is_empty());
    if text.is_none() && envelope.image.is_none() {
        return Ok(());
    }
    if state
        .store
        .count_recent_weixin_commands(Utc::now() - ChronoDuration::minutes(1))
        > MAX_COMMANDS_PER_MINUTE
    {
        return enqueue_text(
            &state,
            &format!("reply-rate-limit-{}", message.id),
            "操作过于频繁，请一分钟后再试。",
        );
    }
    let response = route_command(
        state.clone(),
        &envelope.from_user_id,
        message.remote_message_id.as_deref().unwrap_or(&message.id),
        text.as_deref(),
        envelope.image.as_ref(),
    )
    .await;
    match response {
        Ok(Some(text)) => enqueue_text(&state, &format!("reply-{}", message.id), &text),
        Ok(None) => Ok(()),
        Err(error) => enqueue_text(
            &state,
            &format!("reply-error-{}", message.id),
            &user_error(&error),
        ),
    }
}

async fn route_command(
    state: web::Data<ServerState>,
    _from_user_id: &str,
    inbound_message_id: &str,
    text: Option<&str>,
    image: Option<&WeixinInboundImage>,
) -> Result<Option<String>, String> {
    let Some(text) = text else {
        return Ok(Some(
            "请发送“生成 你的提示词”，需要参考图时可以同一条消息带图。".to_string(),
        ));
    };
    if let Some(result) = route_read_command(&state, text) {
        return result;
    }
    if let Some(result) = route_setting_command(&state, text) {
        return result;
    }
    if let Some(result) = route_action_command(&state, text) {
        return result;
    }
    let prompt = text.strip_prefix("生成 ").map(str::trim);
    let direct_prompt = state.store.get_weixin_preferences().direct_prompt_enabled;
    if prompt.is_some() || direct_prompt {
        let prompt = prompt.unwrap_or(text);
        let reference_urls = reference_urls_for_image(&state, image).await?;
        return create_image_task_with_options(
            &state,
            inbound_message_id,
            prompt,
            None,
            None,
            &reference_urls,
        )
        .await;
    }
    Ok(Some("未识别该操作。回复“帮助”查看可用指令。".to_string()))
}

type RoutedCommand = Option<Result<Option<String>, String>>;

fn route_read_command(state: &ServerState, text: &str) -> RoutedCommand {
    if text == "帮助" || text.eq_ignore_ascii_case("help") {
        let enabled = state.store.get_weixin_preferences().generation_enabled;
        return Some(Ok(Some(help_text(enabled))));
    }
    if text == "账号" {
        return Some(Ok(Some(account_summary(state))));
    }
    if text == "任务" {
        return Some(Ok(Some(task_list(state))));
    }
    if let Some(value) = text.strip_prefix("任务 ") {
        return Some(task_detail(state, value.trim()).map(Some));
    }
    if text == "设置" {
        return Some(Ok(Some(settings_summary(state))));
    }
    if text == "模型" {
        return Some(Ok(Some(model_list(state))));
    }
    None
}

fn route_setting_command(state: &ServerState, text: &str) -> RoutedCommand {
    if let Some(value) = text.strip_prefix("模型 ") {
        return Some(update_model(state, value.trim()).map(Some));
    }
    if let Some(value) = text.strip_prefix("比例 ") {
        return Some(update_ratio(state, value.trim()).map(Some));
    }
    if let Some(value) = text.strip_prefix("张数 ") {
        return Some(update_count(state, value.trim()).map(Some));
    }
    if text == "通知" {
        return Some(Ok(Some(notification_summary(state))));
    }
    text.strip_prefix("通知 ")
        .map(|value| update_notification(state, value.trim()).map(Some))
}

fn route_action_command(state: &ServerState, text: &str) -> RoutedCommand {
    text.strip_prefix("取消 ")
        .map(|value| cancel_task(state, value.trim()).map(Some))
}

async fn create_image_task_with_options(
    state: &ServerState,
    inbound_message_id: &str,
    prompt: &str,
    model_id: Option<&str>,
    ratio: Option<&str>,
    reference_urls: &[String],
) -> Result<Option<String>, String> {
    let prompt = sanitize_text(prompt, MAX_PROMPT_CHARS);
    if prompt.is_empty() {
        return Err("提示词不能为空".to_string());
    }
    if state
        .store
        .count_recent_weixin_tasks(Utc::now() - ChronoDuration::minutes(1))
        >= MAX_IMAGE_TASKS_PER_MINUTE
    {
        return Err("图片任务创建过于频繁，请一分钟后再试".to_string());
    }
    let preferences = state.store.get_weixin_preferences();
    if !preferences.generation_enabled {
        return Err("微信创建图片尚未开启，请先在控制台微信页面开启。".to_string());
    }
    if preferences.default_model_id.trim().is_empty() && model_id.is_none() {
        return Err("请先在控制台微信页面选择默认图片模型。".to_string());
    }
    let body = build_image_request(&preferences, prompt, model_id, ratio, reference_urls)?;
    let model_label = body.model.clone();
    let ratio_label = body.image_aspect_ratio.clone().unwrap_or_default();
    let task_id =
        crate::generation_queue::enqueue_image_job(state, WEIXIN_OWNER_TOKEN, String::new(), body)
            .map_err(|error| error.detail)?;
    state.store.link_weixin_task(inbound_message_id, &task_id);
    Ok(Some(format!(
        "已创建图片任务\n任务：{}\n模型：{}\n比例：{}\n张数：{}",
        short_id(&task_id),
        model_label,
        ratio_label,
        preferences.default_max_images
    )))
}

fn build_image_request(
    preferences: &crate::store::WeixinPreferencesRecord,
    prompt: String,
    model_id: Option<&str>,
    ratio: Option<&str>,
    reference_urls: &[String],
) -> Result<OpenAiImageRequest, String> {
    let mut body = OpenAiImageRequest::empty();
    body.model = model_id
        .unwrap_or(&preferences.default_model_id)
        .trim()
        .to_string();
    body.prompt = Some(Value::String(prompt));
    body.n = Some(preferences.default_max_images.clamp(1, 4));
    body.image_aspect_ratio = Some(
        ratio
            .unwrap_or(&preferences.default_aspect_ratio)
            .trim()
            .to_string(),
    );
    body.async_generation = true;
    body.images = reference_urls
        .iter()
        .map(|url| OpenAiImageRef {
            file_id: None,
            image_url: Some(url.clone()),
        })
        .collect();
    body.validate(ImageEndpoint::Generation)
        .map_err(|(_, _, message)| message)?;
    Ok(body)
}

async fn reference_urls_for_image(
    state: &ServerState,
    image: Option<&WeixinInboundImage>,
) -> Result<Vec<String>, String> {
    let Some(image) = image else {
        return Ok(Vec::new());
    };
    super::inbound_media::upload_reference_image(state, image)
        .await
        .map(|value| vec![value])
}

fn update_model(state: &ServerState, model_id: &str) -> Result<String, String> {
    let exists = state
        .plugin_loader
        .all_models()
        .into_iter()
        .any(|(_, model)| model.task_kind == TaskKind::Image && model.model_id == model_id);
    if !exists {
        return Err("模型不存在或不支持图片生成".to_string());
    }
    update_preferences(state, |value| value.default_model_id = model_id.to_string());
    Ok(format!("默认模型已改为 {model_id}"))
}

fn update_ratio(state: &ServerState, ratio: &str) -> Result<String, String> {
    const ALLOWED: &[&str] = &["auto", "1:1", "9:16", "16:9", "2:1"];
    if !ALLOWED.contains(&ratio) {
        return Err("比例只支持 auto、1:1、9:16、16:9、2:1".to_string());
    }
    update_preferences(state, |value| {
        value.default_aspect_ratio = ratio.to_string()
    });
    Ok(format!("默认比例已改为 {ratio}"))
}

fn update_count(state: &ServerState, value: &str) -> Result<String, String> {
    let count = value
        .parse::<u32>()
        .ok()
        .filter(|count| (1..=4).contains(count))
        .ok_or_else(|| "张数只能是 1 到 4".to_string())?;
    update_preferences(state, |value| value.default_max_images = count);
    Ok(format!("默认张数已改为 {count}"))
}

fn update_notification(state: &ServerState, value: &str) -> Result<String, String> {
    let enabled = match value {
        "开启" => true,
        "关闭" => false,
        _ => return Err("请发送“通知 开启”或“通知 关闭”".to_string()),
    };
    update_preferences(state, |value| {
        value.proactive_notifications_enabled = enabled
    });
    Ok(format!(
        "主动通知已{}",
        if enabled { "开启" } else { "关闭" }
    ))
}

fn update_preferences(
    state: &ServerState,
    update: impl FnOnce(&mut crate::store::WeixinPreferencesRecord),
) {
    let mut preferences = state.store.get_weixin_preferences();
    update(&mut preferences);
    state.store.save_weixin_preferences(preferences);
    state.weixin.wake.notify_waiters();
}

fn cancel_task(state: &ServerState, value: &str) -> Result<String, String> {
    let task = state
        .store
        .find_task_by_id_or_prefix(value)?
        .ok_or_else(|| "没有找到唯一匹配的任务，请输入更完整的任务 ID".to_string())?;
    if matches!(task.status.as_str(), "completed" | "failed" | "cancelled") {
        return Err("任务已经结束，不能取消".to_string());
    }
    let cancelled = state.store.cancel_task_or_generation_job(&task.id)?;
    state.generation_queue_notify.notify_waiters();
    state.weixin.wake.notify_waiters();
    Ok(format!("任务 {} 已取消", short_id(&cancelled.id)))
}

pub(crate) fn enqueue_text(state: &ServerState, client_id: &str, text: &str) -> Result<(), String> {
    state.store.enqueue_weixin_outbound(
        client_id,
        "text",
        &json!({ "text": truncate(text, 900) }).to_string(),
    );
    state.weixin.wake.notify_waiters();
    Ok(())
}

fn account_summary(state: &ServerState) -> String {
    let mut groups = BTreeMap::<String, (usize, usize)>::new();
    for account in state.store.list_accounts(None) {
        let entry = groups.entry(account.platform).or_default();
        entry.1 += 1;
        if account.login_state == "available" {
            entry.0 += 1;
        }
    }
    if groups.is_empty() {
        return "当前没有账号。".to_string();
    }
    let mut lines = vec!["账号概况".to_string()];
    for (platform, (available, total)) in groups {
        lines.push(format!("{platform}：{available}/{total} 可用"));
    }
    lines.join("\n")
}

fn task_list(state: &ServerState) -> String {
    let tasks = state.store.list_tasks(None);
    if tasks.is_empty() {
        return "暂无任务。".to_string();
    }
    let mut lines = vec!["最近任务".to_string()];
    for task in tasks.into_iter().take(10) {
        lines.push(format!(
            "{}  {}  {}",
            short_id(&task.id),
            task_status(&task.status),
            truncate(&task.prompt, 24)
        ));
    }
    lines.join("\n")
}

fn task_detail(state: &ServerState, value: &str) -> Result<String, String> {
    let task = state
        .store
        .find_task_by_id_or_prefix(value)?
        .ok_or_else(|| "没有找到唯一匹配的任务，请输入更完整的任务 ID".to_string())?;
    let mut text = format!(
        "任务详情\n任务：{}\n状态：{}\n模型：{}\n提示词：{}",
        short_id(&task.id),
        task_status(&task.status),
        task.model_id,
        truncate(&task.prompt, 120)
    );
    if let Some(error) = task.error_message.filter(|value| !value.is_empty()) {
        text.push_str(&format!("\n原因：{}", truncate(&error, 100)));
    }
    Ok(text)
}

fn settings_summary(state: &ServerState) -> String {
    let value = state.store.get_weixin_preferences();
    format!(
        "微信创作设置\n创建图片：{}\n直接提示词：{}\n模型：{}\n比例：{}\n张数：{}",
        on_off(value.generation_enabled),
        on_off(value.direct_prompt_enabled),
        empty_as_unset(&value.default_model_id),
        value.default_aspect_ratio,
        value.default_max_images
    )
}

fn model_list(state: &ServerState) -> String {
    let mut lines = vec!["可用图片模型".to_string()];
    for (_, model) in state
        .plugin_loader
        .all_models()
        .into_iter()
        .filter(|(_, model)| model.task_kind == TaskKind::Image)
        .take(20)
    {
        lines.push(format!("{}  {}", model.model_id, model.display_name));
    }
    if lines.len() == 1 {
        "当前没有可用图片模型。".to_string()
    } else {
        lines.join("\n")
    }
}

fn notification_summary(state: &ServerState) -> String {
    let value = state.store.get_weixin_preferences();
    format!(
        "通知设置\n总开关：{}\n无可用账号：{}\n任务失败：{}\n需要人工处理：{}\n任务成功：{}",
        on_off(value.proactive_notifications_enabled),
        on_off(value.notify_no_accounts),
        on_off(value.notify_desktop_tasks),
        on_off(value.notify_account_actions),
        on_off(value.notify_task_successes)
    )
}

fn help_text(generation_enabled: bool) -> String {
    let generation = if generation_enabled {
        "微信生图：已开启\n发送“生成 一只白色陶瓷杯”，任务会进入后台队列。"
    } else {
        "微信生图：未开启\n请先在控制台微信页面开启，并选择默认模型。"
    };
    [
        "微信帮助",
        generation,
        "查询状态：发送“任务”或“任务 任务ID前缀”",
        "修改设置：发送“模型 模型ID”、“比例 9:16”、“张数 1”",
        "通知开关：发送“通知 开启”或“通知 关闭”",
        "取消任务：发送“取消 任务ID前缀”",
    ]
    .join("\n\n")
}

fn sanitize_text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .take(limit)
        .collect::<String>()
        .trim()
        .to_string()
}

fn task_status(status: &str) -> &'static str {
    match status {
        "pending" => "等待中",
        "processing" => "生成中",
        "completed" => "已完成",
        "failed" => "失败",
        "cancelled" => "已取消",
        "verification_required" => "等待验证",
        _ => "未知",
    }
}

fn short_id(value: &str) -> &str {
    value.get(..8).unwrap_or(value)
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn on_off(value: bool) -> &'static str {
    if value {
        "开启"
    } else {
        "关闭"
    }
}

fn empty_as_unset(value: &str) -> &str {
    if value.is_empty() {
        "未设置"
    } else {
        value
    }
}

fn user_error(value: &str) -> String {
    format!("操作失败\n原因：{}", truncate(value, 180))
}

#[cfg(test)]
mod tests {
    #[test]
    fn help_mentions_queue_generation() {
        let text = super::help_text(true);
        assert!(text.contains("微信生图：已开启"));
        assert!(text.contains("后台队列"));
    }
}
