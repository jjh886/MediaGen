use std::time::Duration;

use actix_web::web;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use serde_json::json;

use super::router::process_message;
use super::runtime::hash_code;
use super::types::{InboundMessage, WeixinInboundImage};
use crate::store::WeixinBindingRecord;
use crate::ServerState;

const IDLE_DELAY: Duration = Duration::from_secs(3);
const FAILURE_DELAY: Duration = Duration::from_secs(2);
const BACKOFF_DELAY: Duration = Duration::from_secs(30);

pub(crate) async fn run_monitor(state: web::Data<ServerState>) {
    let mut consecutive_failures = 0u32;
    loop {
        let Some(binding) = load_binding(&state) else {
            wait_or_wake(&state, IDLE_DELAY).await;
            continue;
        };
        if !matches!(binding.status.as_str(), "active" | "pending_activation") {
            wait_or_wake(&state, IDLE_DELAY).await;
            continue;
        }
        let token = match state
            .credential_cipher
            .decrypt(&binding.bot_token_ciphertext)
        {
            Ok(value) => value,
            Err(error) => {
                state.store.mark_weixin_reauth_required(&error.to_string());
                wait_or_wake(&state, BACKOFF_DELAY).await;
                continue;
            }
        };
        let client = match state.weixin.client() {
            Ok(value) => value,
            Err(_) => {
                wait_or_wake(&state, BACKOFF_DELAY).await;
                continue;
            }
        };
        match client
            .get_updates(&binding.base_url, &token, &binding.get_updates_buf)
            .await
        {
            Ok(response) if response.ret.unwrap_or(0) == 0 => {
                consecutive_failures = 0;
                if let Err(error) = persist_updates(&state, &binding, response).await {
                    eprintln!("[mediagen-server] 保存微信入站消息失败: {error}");
                    wait_or_wake(&state, FAILURE_DELAY).await;
                }
            }
            Ok(response) => {
                let error = format!(
                    "getupdates ret={} errcode={} {}",
                    response.ret.unwrap_or_default(),
                    response.errcode.unwrap_or_default(),
                    response.errmsg.unwrap_or_default()
                );
                if response.errcode == Some(-14) {
                    state.store.mark_weixin_reauth_required(&error);
                }
                consecutive_failures += 1;
                wait_or_wake(&state, failure_delay_for_count(consecutive_failures)).await;
            }
            Err(error) => {
                consecutive_failures += 1;
                eprintln!("[mediagen-server] 微信长轮询失败，将自动重试: {error}");
                wait_or_wake(&state, failure_delay_for_count(consecutive_failures)).await;
            }
        }
    }
}

async fn persist_updates(
    state: &ServerState,
    binding: &WeixinBindingRecord,
    response: super::types::GetUpdatesResponse,
) -> Result<(), String> {
    let cursor = response
        .get_updates_buf
        .unwrap_or_else(|| binding.get_updates_buf.clone());
    let mut messages = Vec::new();
    let mut activated = binding.status == "active";
    for message in response.msgs {
        let Some((remote_id, text, image)) = parse_inbound_payload(state, binding, &message)?
        else {
            continue;
        };
        persist_context_token(state, &message)?;
        if activate_message(state, binding, &message, text.as_deref(), activated)? {
            activated = true;
            continue;
        }
        if activated {
            messages.push((
                remote_id,
                json!({
                    "from_user_id": message.from_user_id.unwrap_or_default(),
                    "text": text,
                    "image": image,
                })
                .to_string(),
            ));
        }
    }
    state.store.save_weixin_inbound_batch(&messages, &cursor);
    if !messages.is_empty() {
        state.store.touch_weixin_inbound();
        state.weixin.wake.notify_waiters();
    }
    Ok(())
}

fn parse_inbound_payload(
    state: &ServerState,
    binding: &WeixinBindingRecord,
    message: &InboundMessage,
) -> Result<Option<(String, Option<String>, Option<WeixinInboundImage>)>, String> {
    if !is_authorized_message(binding, message) {
        return Ok(None);
    }
    let Some(remote_id) = message.message_id.map(|value| value.to_string()) else {
        return Ok(None);
    };
    let text = inbound_text(message);
    let image = inbound_image(state, message)?;
    if text.is_none() && image.is_none() {
        return Ok(None);
    }
    Ok(Some((remote_id, text, image)))
}

fn persist_context_token(state: &ServerState, message: &InboundMessage) -> Result<(), String> {
    let Some(context_token) = message
        .context_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(());
    };
    let encrypted = state
        .credential_cipher
        .encrypt(context_token)
        .map_err(|error| error.to_string())?;
    state.store.update_weixin_context(&encrypted);
    Ok(())
}

fn activate_message(
    state: &ServerState,
    binding: &WeixinBindingRecord,
    message: &InboundMessage,
    text: Option<&str>,
    already_active: bool,
) -> Result<bool, String> {
    let Some(text) = text.filter(|_| !already_active) else {
        return Ok(false);
    };
    if !activate_if_valid(state, binding, message, text)? {
        return Ok(false);
    }
    super::router::enqueue_text(
        state,
        &format!("binding-success-{}", Utc::now().timestamp_millis()),
        "微信绑定成功，可以发送“帮助”查看操作。",
    )?;
    Ok(true)
}

fn activate_if_valid(
    state: &ServerState,
    binding: &WeixinBindingRecord,
    message: &InboundMessage,
    text: &str,
) -> Result<bool, String> {
    if binding.status != "pending_activation" {
        return Ok(false);
    }
    let valid_code = super::activation::parse_code(text)
        .is_some_and(|code| hash_code(code) == binding.activation_code_hash);
    let unexpired = binding
        .activation_expires_at
        .as_deref()
        .and_then(crate::store_weixin::parse_time)
        .is_some_and(|expires_at| expires_at > Utc::now());
    let Some(context_token) = message
        .context_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(false);
    };
    if !valid_code || !unexpired {
        return Ok(false);
    }
    let encrypted = state
        .credential_cipher
        .encrypt(context_token)
        .map_err(|error| error.to_string())?;
    state.store.activate_weixin_binding(&encrypted);
    Ok(true)
}

fn is_authorized_message(binding: &WeixinBindingRecord, message: &InboundMessage) -> bool {
    message.message_type == Some(1)
        && message.from_user_id.as_deref() == Some(binding.ilink_user_id.as_str())
        && message
            .group_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
}

fn inbound_image(
    state: &ServerState,
    message: &InboundMessage,
) -> Result<Option<WeixinInboundImage>, String> {
    let Some(image) = message
        .item_list
        .iter()
        .find(|item| item.item_type == Some(2))
        .and_then(|item| item.image_item.as_ref())
    else {
        return Ok(None);
    };
    let media = image.media.as_ref().or(image.thumb_media.as_ref());
    let full_url = media.and_then(|value| non_empty(value.full_url.as_deref()));
    let encrypt_query_param =
        media.and_then(|value| non_empty(value.encrypt_query_param.as_deref()));
    if full_url.is_none() && encrypt_query_param.is_none() {
        return Ok(None);
    }
    let raw_key = parse_inbound_aes_key(
        image.aeskey.as_deref(),
        media.and_then(|value| value.aes_key.as_deref()),
    )?;
    let aes_key_ciphertext = raw_key
        .map(|key| state.credential_cipher.encrypt(&STANDARD.encode(key)))
        .transpose()
        .map_err(|error| error.to_string())?;
    Ok(Some(WeixinInboundImage {
        full_url: full_url.map(str::to_string),
        encrypt_query_param: encrypt_query_param.map(str::to_string),
        aes_key_ciphertext,
    }))
}

fn inbound_text(message: &InboundMessage) -> Option<String> {
    let value = message
        .item_list
        .iter()
        .find(|item| item.item_type == Some(1))?
        .text_item
        .as_ref()?
        .text
        .as_deref()?
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .take(4_000)
        .collect::<String>()
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn parse_inbound_aes_key(
    hex_key: Option<&str>,
    base64_key: Option<&str>,
) -> Result<Option<Vec<u8>>, String> {
    if let Some(value) = non_empty(hex_key) {
        return parse_hex_key(value).map(Some);
    }
    let Some(value) = non_empty(base64_key) else {
        return Ok(None);
    };
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| "微信参考图密钥格式无效".to_string())?;
    if decoded.len() == 16 {
        return Ok(Some(decoded));
    }
    if decoded.len() == 32 {
        let hex =
            std::str::from_utf8(&decoded).map_err(|_| "微信参考图密钥格式无效".to_string())?;
        return parse_hex_key(hex).map(Some);
    }
    Err("微信参考图密钥长度无效".to_string())
}

fn parse_hex_key(value: &str) -> Result<Vec<u8>, String> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("微信参考图密钥格式无效".to_string());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| "微信参考图密钥格式无效".to_string())
        })
        .collect()
}

pub(crate) async fn run_inbound_worker(state: web::Data<ServerState>) {
    loop {
        match state.store.next_weixin_message("inbound", Utc::now()) {
            Some(message) => {
                let result = process_message(state.clone(), message.clone()).await;
                if let Err(error) = result {
                    state.store.finish_weixin_message(&message.id, "failed");
                    eprintln!("[mediagen-server] 处理微信指令失败: {error}");
                } else {
                    state.store.finish_weixin_message(&message.id, "sent");
                }
            }
            None => wait_or_wake(&state, IDLE_DELAY).await,
        }
    }
}

fn load_binding(state: &ServerState) -> Option<WeixinBindingRecord> {
    state.store.get_weixin_binding()
}

fn failure_delay_for_count(count: u32) -> Duration {
    if count >= 3 {
        BACKOFF_DELAY
    } else {
        FAILURE_DELAY
    }
}

async fn wait_or_wake(state: &web::Data<ServerState>, duration: Duration) {
    tokio::select! {
        _ = state.weixin.wake.notified() => {}
        _ = tokio::time::sleep(duration) => {}
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_hex_weixin_image_key() {
        let key = super::parse_inbound_aes_key(Some("000102030405060708090a0b0c0d0e0f"), None)
            .unwrap()
            .unwrap();
        assert_eq!(key.len(), 16);
        assert_eq!(key[15], 15);
    }
}
