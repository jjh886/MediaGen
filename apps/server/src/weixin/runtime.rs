use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, Notify};

use super::client::{fixed_base_url, render_qr_data_url, WeixinIlinkClient};
use super::types::{BindingActionResult, QrStatusResponse};
use crate::store::WeixinBindingRecord;
use crate::store_weixin::WEIXIN_BINDING_ID;
use crate::ServerState;

const SESSION_TTL_MINUTES: i64 = 8;
const MAX_QR_REFRESHES: u8 = 3;

struct LoginSession {
    qrcode: String,
    qr_content: String,
    polling_base_url: String,
    pending_verify_code: Option<String>,
    started_at: chrono::DateTime<Utc>,
    refresh_count: u8,
}

struct ConfirmedBinding {
    bot_token: String,
    bot_id: String,
    user_id: String,
    base_url: String,
}

pub(crate) struct WeixinRuntime {
    pub(crate) client: Option<Arc<WeixinIlinkClient>>,
    sessions: Mutex<HashMap<String, LoginSession>>,
    pub(crate) wake: Notify,
}

impl WeixinRuntime {
    pub(crate) fn new() -> Result<Self, String> {
        Ok(Self {
            client: Some(Arc::new(WeixinIlinkClient::new()?)),
            sessions: Mutex::new(HashMap::new()),
            wake: Notify::new(),
        })
    }

    pub(crate) fn offline() -> Self {
        Self {
            client: None,
            sessions: Mutex::new(HashMap::new()),
            wake: Notify::new(),
        }
    }

    pub(crate) fn client(&self) -> Result<Arc<WeixinIlinkClient>, String> {
        self.client
            .clone()
            .ok_or_else(|| "微信网络客户端不可用".to_string())
    }

    pub(crate) async fn start_binding(
        &self,
        state: &ServerState,
    ) -> Result<BindingActionResult, String> {
        if let Some(result) = renew_pending_binding(state)? {
            self.wake.notify_waiters();
            return Ok(result);
        }
        let local_tokens = state
            .store
            .get_weixin_binding()
            .and_then(|binding| {
                state
                    .credential_cipher
                    .decrypt(&binding.bot_token_ciphertext)
                    .ok()
            })
            .into_iter()
            .take(10)
            .collect();
        let qr = self.client()?.fetch_qr_code(local_tokens).await?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let qr_image = render_qr_data_url(&qr.qrcode_img_content)?;
        self.sessions.lock().await.insert(
            session_id.clone(),
            LoginSession {
                qrcode: qr.qrcode,
                qr_content: qr.qrcode_img_content,
                polling_base_url: fixed_base_url().to_string(),
                pending_verify_code: None,
                started_at: Utc::now(),
                refresh_count: 1,
            },
        );
        Ok(action_result(
            "wait",
            "请使用微信扫描二维码",
            Some(session_id),
            Some(qr_image),
            None,
        ))
    }

    pub(crate) async fn submit_verify_code(
        &self,
        session_id: &str,
        code: &str,
    ) -> Result<BindingActionResult, String> {
        let code = code.trim();
        if code.is_empty() || code.len() > 12 || !code.chars().all(|value| value.is_ascii_digit()) {
            return Err("扫码验证码格式无效".to_string());
        }
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "扫码会话不存在或已过期".to_string())?;
        session.pending_verify_code = Some(code.to_string());
        Ok(action_result(
            "verifying",
            "正在验证",
            Some(session_id.to_string()),
            None,
            None,
        ))
    }

    pub(crate) async fn cancel_binding(&self, session_id: &str) -> BindingActionResult {
        self.sessions.lock().await.remove(session_id);
        action_result("cancelled", "已取消扫码绑定", None, None, None)
    }

    pub(crate) async fn clear_sessions(&self) {
        self.sessions.lock().await.clear();
    }

    pub(crate) async fn poll_binding(
        &self,
        state: &ServerState,
        session_id: &str,
    ) -> Result<BindingActionResult, String> {
        let (base_url, qrcode, verify_code) = self.poll_input(session_id).await?;
        let status = self
            .client()?
            .poll_qr_status(&base_url, &qrcode, verify_code.as_deref())
            .await?;
        match status.status.as_str() {
            "wait" => Ok(action_result(
                "wait",
                "等待扫码",
                Some(session_id.to_string()),
                None,
                None,
            )),
            "scaned" => Ok(action_result(
                "scanned",
                "已扫码，等待手机确认",
                Some(session_id.to_string()),
                None,
                None,
            )),
            "need_verifycode" => Ok(action_result(
                "need_verify_code",
                "请输入手机微信显示的数字",
                Some(session_id.to_string()),
                None,
                None,
            )),
            "verify_code_blocked" => {
                self.sessions.lock().await.remove(session_id);
                Ok(action_result(
                    "blocked",
                    "扫码验证码多次错误，请稍后重新扫码",
                    None,
                    None,
                    None,
                ))
            }
            "expired" => self.refresh_qr(session_id).await,
            "scaned_but_redirect" => self.apply_redirect(session_id, status).await,
            "binded_redirect" => {
                self.sessions.lock().await.remove(session_id);
                recover_redirected_binding(state)
            }
            "confirmed" => self.confirm_binding(state, session_id, status).await,
            other => Err(format!("未知微信扫码状态: {other}")),
        }
    }

    async fn poll_input(
        &self,
        session_id: &str,
    ) -> Result<(String, String, Option<String>), String> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "扫码会话不存在或已过期".to_string())?;
        if Utc::now() - session.started_at > ChronoDuration::minutes(SESSION_TTL_MINUTES) {
            sessions.remove(session_id);
            return Err("扫码已超时，请重新绑定".to_string());
        }
        Ok((
            session.polling_base_url.clone(),
            session.qrcode.clone(),
            session.pending_verify_code.clone(),
        ))
    }

    async fn refresh_qr(&self, session_id: &str) -> Result<BindingActionResult, String> {
        let refresh_count = self
            .sessions
            .lock()
            .await
            .get(session_id)
            .map(|session| session.refresh_count)
            .ok_or_else(|| "扫码会话不存在或已过期".to_string())?;
        if refresh_count >= MAX_QR_REFRESHES {
            self.sessions.lock().await.remove(session_id);
            return Ok(action_result(
                "expired",
                "二维码多次过期，请重新发起绑定",
                None,
                None,
                None,
            ));
        }
        let qr = self.client()?.fetch_qr_code(Vec::new()).await?;
        let qr_image = render_qr_data_url(&qr.qrcode_img_content)?;
        if let Some(session) = self.sessions.lock().await.get_mut(session_id) {
            session.qrcode = qr.qrcode;
            session.qr_content = qr.qrcode_img_content;
            session.polling_base_url = fixed_base_url().to_string();
            session.pending_verify_code = None;
            session.started_at = Utc::now();
            session.refresh_count += 1;
        }
        Ok(action_result(
            "wait",
            "二维码已刷新，请重新扫描",
            Some(session_id.to_string()),
            Some(qr_image),
            None,
        ))
    }

    async fn apply_redirect(
        &self,
        session_id: &str,
        status: QrStatusResponse,
    ) -> Result<BindingActionResult, String> {
        let host = status
            .redirect_host
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "微信扫码重定向地址缺失".to_string())?;
        let redirected = format!("https://{}", host.trim());
        super::client::validate_api_base_url(&redirected)?;
        if let Some(session) = self.sessions.lock().await.get_mut(session_id) {
            session.polling_base_url = redirected;
        }
        Ok(action_result(
            "scanned",
            "已扫码，继续确认",
            Some(session_id.to_string()),
            None,
            None,
        ))
    }

    async fn confirm_binding(
        &self,
        state: &ServerState,
        session_id: &str,
        status: QrStatusResponse,
    ) -> Result<BindingActionResult, String> {
        let confirmed = parse_confirmed_binding(status)?;
        let activation_code = super::activation::new_code();
        let binding = build_pending_binding(state, confirmed, &activation_code)?;
        state.store.save_weixin_binding(binding);
        self.sessions.lock().await.remove(session_id);
        self.wake.notify_waiters();
        Ok(action_result(
            "pending_activation",
            "请在微信中发送 6 位数字",
            None,
            None,
            Some(activation_code),
        ))
    }
}

pub(crate) fn start_workers(state: actix_web::web::Data<ServerState>) {
    let monitor_state = state.clone();
    actix_web::rt::spawn(async move {
        super::monitor::run_monitor(monitor_state).await;
    });
    let inbound_state = state.clone();
    actix_web::rt::spawn(async move {
        super::monitor::run_inbound_worker(inbound_state).await;
    });
    let outbound_state = state.clone();
    actix_web::rt::spawn(async move {
        super::outbound::run_outbound_worker(outbound_state).await;
    });
    let notification_state = state.clone();
    actix_web::rt::spawn(async move {
        super::notification_worker::run_notification_worker(notification_state).await;
    });
    actix_web::rt::spawn(async move {
        super::delivery::run_task_delivery_worker(state).await;
    });
}

pub(crate) fn hash_code(code: &str) -> String {
    format!("{:x}", Sha256::digest(code.as_bytes()))
}

fn renew_pending_binding(state: &ServerState) -> Result<Option<BindingActionResult>, String> {
    let Some(binding) = state
        .store
        .get_weixin_binding()
        .filter(|value| value.status == "pending_activation")
    else {
        return Ok(None);
    };
    if !has_local_bot_token(state, &binding) {
        return Ok(None);
    }
    renew_activation(state).map(Some)
}

fn recover_redirected_binding(state: &ServerState) -> Result<BindingActionResult, String> {
    let Some(binding) = state.store.get_weixin_binding() else {
        return Ok(action_result(
            "reauth_required",
            "服务端已有绑定，但本机没有可恢复的凭据，请先移除绑定",
            None,
            None,
            None,
        ));
    };
    if !has_local_bot_token(state, &binding) {
        return Ok(action_result(
            "reauth_required",
            "本机微信凭据不可用，请先移除绑定",
            None,
            None,
            None,
        ));
    }
    if binding.status == "active" && has_local_context(state, &binding) {
        return Ok(action_result("active", "该微信已绑定", None, None, None));
    }
    renew_activation(state)
}

fn renew_activation(state: &ServerState) -> Result<BindingActionResult, String> {
    let code = super::activation::new_code();
    state.store.renew_weixin_activation(
        &hash_code(&code),
        Utc::now() + ChronoDuration::minutes(10),
        Utc::now(),
    )?;
    Ok(action_result(
        "pending_activation",
        "请在微信中发送 6 位数字",
        None,
        None,
        Some(code),
    ))
}

fn has_local_bot_token(state: &ServerState, binding: &WeixinBindingRecord) -> bool {
    !binding.bot_token_ciphertext.is_empty()
        && state
            .credential_cipher
            .decrypt(&binding.bot_token_ciphertext)
            .is_ok()
}

fn has_local_context(state: &ServerState, binding: &WeixinBindingRecord) -> bool {
    !binding.context_token_ciphertext.is_empty()
        && state
            .credential_cipher
            .decrypt(&binding.context_token_ciphertext)
            .is_ok()
}

fn parse_confirmed_binding(status: QrStatusResponse) -> Result<ConfirmedBinding, String> {
    let base_url = status
        .baseurl
        .unwrap_or_else(|| fixed_base_url().to_string());
    super::client::validate_api_base_url(&base_url)?;
    Ok(ConfirmedBinding {
        bot_token: required(status.bot_token, "微信确认响应缺少 Bot Token")?,
        bot_id: required(status.ilink_bot_id, "微信确认响应缺少 Bot ID")?,
        user_id: required(status.ilink_user_id, "微信确认响应缺少用户 ID")?,
        base_url,
    })
}

fn build_pending_binding(
    state: &ServerState,
    confirmed: ConfirmedBinding,
    activation_code: &str,
) -> Result<WeixinBindingRecord, String> {
    let now = Utc::now();
    let encrypted_token = state
        .credential_cipher
        .encrypt(&confirmed.bot_token)
        .map_err(|error| error.to_string())?;
    let created_at = state
        .store
        .get_weixin_binding()
        .map(|binding| binding.created_at)
        .unwrap_or_else(|| now.to_rfc3339());
    Ok(WeixinBindingRecord {
        id: WEIXIN_BINDING_ID.to_string(),
        status: "pending_activation".to_string(),
        ilink_bot_id: confirmed.bot_id,
        ilink_user_id: confirmed.user_id,
        base_url: confirmed.base_url,
        bot_token_ciphertext: encrypted_token,
        context_token_ciphertext: String::new(),
        get_updates_buf: String::new(),
        activation_code_hash: hash_code(activation_code),
        activation_expires_at: Some((now + ChronoDuration::minutes(10)).to_rfc3339()),
        last_inbound_at: None,
        last_outbound_at: None,
        created_at,
        updated_at: now.to_rfc3339(),
    })
}

fn required(value: Option<String>, message: &str) -> Result<String, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| message.to_string())
}

fn action_result(
    status: &str,
    message: &str,
    session_id: Option<String>,
    qr_image: Option<String>,
    activation_code: Option<String>,
) -> BindingActionResult {
    BindingActionResult {
        status: status.to_string(),
        message: message.to_string(),
        session_id,
        qr_image,
        activation_code,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn activation_code_hash_is_stable_without_plaintext() {
        let hash = super::hash_code("482913");
        assert_eq!(hash, super::hash_code("482913"));
        assert!(!hash.contains("482913"));
    }
}
