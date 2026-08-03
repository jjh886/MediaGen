use std::time::Duration;

use actix_web::web;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{json, Value};

use super::media::{upload_and_send_image, ImageSendContext};
use crate::store::WeixinMessageRecord;
use crate::ServerState;

const IDLE_DELAY: Duration = Duration::from_secs(3);

pub(crate) async fn run_outbound_worker(state: web::Data<ServerState>) {
    loop {
        match state.store.next_weixin_message("outbound", Utc::now()) {
            Some(message) => {
                let result = send_outbound(&state, &message).await;
                finish_outbound_attempt(&state, &message, result);
            }
            None => wait_or_wake(&state).await,
        }
    }
}

async fn send_outbound(state: &ServerState, message: &WeixinMessageRecord) -> Result<(), String> {
    let binding = state
        .store
        .get_weixin_binding()
        .ok_or_else(|| "微信尚未绑定".to_string())?;
    if binding.status != "active" {
        return Err("微信绑定当前不可用".to_string());
    }
    let token = state
        .credential_cipher
        .decrypt(&binding.bot_token_ciphertext)
        .map_err(|error| error.to_string())?;
    let context_token = state
        .credential_cipher
        .decrypt(&binding.context_token_ciphertext)
        .map_err(|error| error.to_string())?;
    let content: Value = serde_json::from_str(&message.content_json)
        .map_err(|error| format!("微信发件箱数据无效: {error}"))?;
    match message.message_type.as_str() {
        "text" => send_text(state, message, &binding, &token, &context_token, &content).await,
        "image" => send_image(state, message, &binding, &token, &context_token, &content).await,
        _ => Err("不支持的微信消息类型".to_string()),
    }
}

async fn send_text(
    state: &ServerState,
    message: &WeixinMessageRecord,
    binding: &crate::store::WeixinBindingRecord,
    token: &str,
    context_token: &str,
    content: &Value,
) -> Result<(), String> {
    let text = content["text"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "微信文字消息为空".to_string())?;
    state
        .weixin
        .client()?
        .send_text(
            &binding.base_url,
            token,
            &binding.ilink_user_id,
            context_token,
            &message.client_id,
            text,
        )
        .await
}

async fn send_image(
    state: &ServerState,
    message: &WeixinMessageRecord,
    binding: &crate::store::WeixinBindingRecord,
    token: &str,
    context_token: &str,
    content: &Value,
) -> Result<(), String> {
    let image_source = content["path"]
        .as_str()
        .or_else(|| content["url"].as_str())
        .or_else(|| content["dataUrl"].as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "微信图片消息地址为空".to_string())?;
    let client = state.weixin.client()?;
    upload_and_send_image(
        &client,
        ImageSendContext {
            base_url: &binding.base_url,
            bot_token: token,
            to_user_id: &binding.ilink_user_id,
            context_token,
            client_id: &message.client_id,
            image_source,
            workspace_dir: &state.file_root,
        },
    )
    .await
}

fn finish_outbound_attempt(
    state: &ServerState,
    message: &WeixinMessageRecord,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => {
            state.store.finish_weixin_message(&message.id, "sent");
            state.store.touch_weixin_outbound();
        }
        Err(error) if message.attempt_count >= 9 => {
            state.store.finish_weixin_message(&message.id, "failed");
            eprintln!(
                "[mediagen-server] 微信消息多次发送失败: client={} error={error}",
                message.client_id
            );
            enqueue_image_fallback(state, message);
        }
        Err(error) => {
            if error.contains("HTTP 401") || error.contains("ret=-14") {
                state.store.mark_weixin_reauth_required(&error);
            }
            let seconds = (2_i64.pow((message.attempt_count as u32).min(7))).clamp(2, 300);
            state.store.retry_weixin_message(
                &message.id,
                Utc::now() + ChronoDuration::seconds(seconds),
                &error,
            );
        }
    }
}

fn enqueue_image_fallback(state: &ServerState, message: &WeixinMessageRecord) {
    if message.message_type != "image" {
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(&message.content_json) else {
        return;
    };
    let text = value["url"]
        .as_str()
        .map(|url| format!("图片发送失败，可通过链接查看：{url}"))
        .unwrap_or_else(|| "图片发送失败，请在控制台任务记录中查看。".to_string());
    state.store.enqueue_weixin_outbound(
        &format!("{}-fallback", message.client_id),
        "text",
        &json!({ "text": text }).to_string(),
    );
    state.weixin.wake.notify_waiters();
}

async fn wait_or_wake(state: &web::Data<ServerState>) {
    tokio::select! {
        _ = state.weixin.wake.notified() => {}
        _ = tokio::time::sleep(IDLE_DELAY) => {}
    }
}
