use crate::{
    protocol_api::{error, run_plugin},
    ServerState,
};
use actix_web::{web, HttpRequest, HttpResponse};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Default)]
pub(crate) struct Sessions(Arc<Mutex<HashMap<String, Session>>>);
#[derive(Clone)]
struct Session {
    owner: String,
    incoming: mpsc::Sender<Message>,
    outgoing: broadcast::Sender<Message>,
    controller: Arc<AtomicBool>,
    close_event: Value,
}

struct ControllerLease(Arc<AtomicBool>);
impl Drop for ControllerLease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/v1/live/sessions", web::post().to(create))
        .route(
            "/v1/live/sessions/{call_id}/attach",
            web::get().to(sideband_path),
        )
        .route("/v1/live/sessions/{call_id}/hangup", web::post().to(hangup))
        .route("/v1/live", web::post().to(create))
        .route("/v1/live/{call_id}", web::get().to(sideband_path));
}

async fn create(
    state: web::Data<ServerState>,
    request: HttpRequest,
    mut payload: web::Payload,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let mut body = if request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("multipart/form-data"))
    {
        match crate::audio_api::multipart(&request, payload, 2 * 1024 * 1024).await {
            Ok(body) => body,
            Err(message) => return error(400, "invalid_request", &message),
        }
    } else {
        let mut bytes = Vec::new();
        while let Some(chunk) = payload.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => return error(400, "invalid_body", "读取请求失败"),
            };
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return error(413, "body_too_large", "请求过大");
            }
            bytes.extend_from_slice(&chunk);
        }
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(body) => body,
            Err(_) => return error(400, "invalid_json", "请求体不是有效 JSON"),
        }
    };
    let live_session = request.path() == "/v1/live/sessions";
    if live_session {
        if body["transport"]["type"] != "webrtc" {
            return error(400, "unsupported_transport", "当前模型只支持 WebRTC 主连接");
        }
        body["sdp"] = body["transport"]["sdp"].clone();
        if let Some(object) = body.as_object_mut() {
            object.remove("transport");
        }
    }
    if let Some(session) = body["session"].as_str() {
        body["session"] = match serde_json::from_str(session) {
            Ok(session) => session,
            Err(_) => return error(400, "invalid_session", "session 不是有效 JSON"),
        };
    }
    let Some(model_id) = body["session"]["model"].as_str().map(ToOwned::to_owned) else {
        return error(400, "missing_model", "session 缺少 model");
    };
    if body["sdp"]
        .as_str()
        .is_none_or(|sdp| !sdp.trim().starts_with("v=0"))
    {
        return error(400, "invalid_sdp", "缺少有效的 SDP offer");
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let Some((manifest, model, _)) = crate::model_api::resolve(&state, &model_id, &key) else {
        return error(404, "model_not_found", "实时模型不存在");
    };
    if model.task_kind != mediagen_core::TaskKind::Realtime {
        return error(400, "unsupported_endpoint", "该模型不支持实时会话");
    }
    let task_id = crate::request_identity::id(&request);
    let (account, lease) = match crate::protocol_task::reserve(
        &state, &manifest, &model_id, "realtime", &key, &task_id, None,
    ) {
        Ok(value) => value,
        Err(_) => return error(429, "account_unavailable", "没有可用账号或账号代理不可用"),
    };
    let execution = run_plugin(
        &state,
        &manifest,
        &account,
        "create_realtime",
        body,
        None,
        &task_id,
        Value::Null,
    );
    let result = tokio::select! {
        result = execution => result,
        _ = crate::protocol_task::cancelled(&state.store, &task_id) => return error(409, "task_cancelled", "请求已取消"),
    };
    let result = match result {
        Ok(result) => result,
        Err(message) => {
            let context = crate::media_execution::MediaFlowContext {
                manifest: &manifest,
                model: &model,
                account: account.clone(),
                task_id: task_id.clone(),
            };
            crate::media_execution::classify_and_apply_failure(&state, &context, &message);
            state.store.fail_task(
                &task_id,
                "实时会话建立失败，需检查设备证明与账号权限".into(),
            );
            return error(
                503,
                "realtime_unavailable",
                "实时会话不可用，需要受支持设备、官方应用及账号权限",
            );
        }
    };
    let call_id = result["call_id"].as_str().unwrap_or("").to_string();
    if call_id.is_empty()
        || call_id.len() > 256
        || !call_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        state.store.fail_task(&task_id, "实时通话 ID 无效".into());
        return error(502, "invalid_call", "上游通话 ID 无效");
    }
    let valid_connection = result["connection"]["url"]
        .as_str()
        .and_then(|value| reqwest::Url::parse(value).ok())
        .is_some_and(|url| matches!(url.scheme(), "ws" | "wss") && url.host_str().is_some());
    if !valid_connection {
        return error(502, "invalid_connection", "插件未返回有效的实时连接地址");
    }
    let owner = crate::access_control::hash_secret(&key);
    let expires_at = chrono::Utc::now().timestamp() + 3600;
    let record = json!({"owner":owner,"result":result,"expires_at":expires_at});
    let encrypted = match state.credential_cipher.encrypt(&record.to_string()) {
        Ok(value) => value,
        Err(_) => return error(500, "session_storage_failed", "实时会话状态保存失败"),
    };
    state
        .store
        .save_task_progress(&task_id, json!({"live_ciphertext":encrypted}));
    spawn_session(state.clone(), account, task_id.clone(), lease, record);
    let request_id = task_id;
    if live_session {
        return HttpResponse::Created()
            .insert_header(("x-request-id", request_id))
            .json(
                json!({"session":{"id":call_id},"transport":{"type":"webrtc","sdp":result["sdp"]}}),
            );
    }
    HttpResponse::Ok()
        .insert_header(("location", format!("/v1/live/{call_id}")))
        .insert_header(("x-request-id", request_id))
        .content_type("application/sdp")
        .body(result["sdp"].as_str().unwrap_or("").to_string())
}

fn spawn_session(
    state: web::Data<ServerState>,
    account: crate::store::AccountRecord,
    task_id: String,
    lease: crate::protocol_task::Lease,
    record: Value,
) {
    let result = &record["result"];
    let call_id = result["call_id"].as_str().unwrap_or_default().to_string();
    let (incoming, receiver) = mpsc::channel(16);
    let (outgoing, _) = broadcast::channel(32);
    let session = Session {
        owner: record["owner"].as_str().unwrap_or_default().into(),
        incoming,
        outgoing: outgoing.clone(),
        controller: Arc::new(AtomicBool::new(false)),
        close_event: result["close_event"].clone(),
    };
    state
        .realtime
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(call_id.clone(), session);
    let close = result["close_event"].clone();
    let terminal = result["terminal_events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect();
    let context = crate::realtime_transport::DriveContext {
        store: state.store.clone(),
        task_id: task_id.clone(),
        account: account.clone(),
        connection: result["connection"].clone(),
        expires_at: record["expires_at"].as_i64().unwrap_or(0),
    };
    tokio::spawn(async move {
        let _lease = lease;
        let outcome =
            crate::realtime_transport::drive(receiver, outgoing, close, terminal, context).await;
        state
            .realtime
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&call_id);
        state.store.mark_account_usage_for(&account.id, "realtime");
        if state
            .store
            .task_by_id(&task_id)
            .is_some_and(|task| task.status == "cancelled")
        {
            state.store.clear_task_progress(&task_id);
            return;
        }
        match outcome {
            Ok(()) => {
                state
                    .store
                    .complete_task(&task_id, json!({"call_id":call_id,"status":"closed"}));
            }
            Err(message) => {
                state.store.fail_task(&task_id, message);
            }
        }
    });
}

pub(crate) fn recover_sessions(state: web::Data<ServerState>) {
    actix_web::rt::spawn(async move {
        for task in state.store.list_tasks(None).into_iter().filter(|task| {
            task.task_kind == "realtime"
                && (task.status == "processing"
                    || (task.status == "cancelled"
                        && task
                            .result_data
                            .as_deref()
                            .is_some_and(|raw| raw.contains("live_ciphertext"))))
        }) {
            let restored = async {
                let progress: Value =
                    serde_json::from_str(task.result_data.as_deref().ok_or("会话记录缺失")?)
                        .map_err(|_| "会话记录无效")?;
                let raw = state
                    .credential_cipher
                    .decrypt(progress["live_ciphertext"].as_str().ok_or("会话凭据缺失")?)?;
                let record: Value = serde_json::from_str(&raw).map_err(|_| "会话记录无效")?;
                if record["expires_at"].as_i64().unwrap_or(0) <= chrono::Utc::now().timestamp() {
                    return Err("会话已超过最大时长".into());
                }
                let account = state
                    .store
                    .account_by_id(task.account_id.as_deref().ok_or("会话账号缺失")?)
                    .ok_or("会话账号已删除")?;
                // 代理暂不可用时仍恢复租约，由观察连接等待代理恢复，避免提前释放并发。
                Ok::<_, String>((account, record))
            }
            .await;
            match restored {
                Ok((account, record)) => {
                    let lease =
                        crate::protocol_task::Lease::new(state.store.clone(), task.id.clone());
                    spawn_session(state.clone(), account, task.id, lease, record);
                }
                Err(_) => {
                    state
                        .store
                        .fail_task(&task.id, "实时会话无法恢复或已过期".into());
                    state.store.clear_task_progress(&task.id);
                }
            }
        }
    });
}

fn session_for(state: &ServerState, request: &HttpRequest, id: &str) -> Option<Session> {
    let key = crate::routes::api_key_from_request(request)?;
    state
        .realtime
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(id)
        .filter(|session| session.owner == crate::access_control::hash_secret(&key))
        .cloned()
}

async fn hangup(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let Some(session) = session_for(&state, &request, &path) else {
        return error(404, "call_not_found", "通话不存在");
    };
    if !matches!(
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            session
                .incoming
                .send(Message::Text(session.close_event.to_string().into()))
        )
        .await,
        Ok(Ok(()))
    ) {
        return error(410, "call_closed", "通话已关闭");
    }
    HttpResponse::Ok().finish()
}
async fn sideband_path(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
    path: web::Path<String>,
) -> HttpResponse {
    relay(state, request, payload, path.into_inner()).await
}

async fn relay(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
    id: String,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    if id.is_empty() {
        return error(
            400,
            "unsupported_transport",
            "当前模型需要先通过 WebRTC 创建通话，再使用 call_id 连接辅助通道",
        );
    }
    let Some(session) = session_for(&state, &request, &id) else {
        return error(404, "call_not_found", "通话不存在");
    };
    if session.controller.swap(true, Ordering::SeqCst) {
        return error(409, "controller_busy", "通话已有辅助连接");
    }
    let (response, mut client, stream) = match actix_ws::handle(&request, payload) {
        Ok(value) => value,
        Err(_) => {
            session.controller.store(false, Ordering::SeqCst);
            return error(400, "invalid_websocket", "WebSocket 握手无效");
        }
    };
    let mut stream = stream
        .max_frame_size(8 * 1024 * 1024)
        .aggregate_continuations()
        .max_continuation_size(8 * 1024 * 1024);
    let mut outgoing = session.outgoing.subscribe();
    actix_web::rt::spawn(async move {
        let _controller = ControllerLease(session.controller.clone());
        loop {
            tokio::select! {
                incoming=stream.next()=>match incoming{
                    Some(Ok(actix_ws::AggregatedMessage::Text(text)))=>{if text.len()>8*1024*1024||session.incoming.send(Message::Text(text.to_string().into())).await.is_err(){break;}},
                    Some(Ok(actix_ws::AggregatedMessage::Binary(bytes)))=>{if bytes.len()>8*1024*1024||session.incoming.send(Message::Binary(bytes)).await.is_err(){break;}},
                    Some(Ok(actix_ws::AggregatedMessage::Ping(bytes)))=>{if client.pong(&bytes).await.is_err(){break;}},
                    Some(Ok(actix_ws::AggregatedMessage::Pong(_)))=>{},
                    _=>break,
                },
                event=outgoing.recv()=>match event{
                    Ok(Message::Text(text))=>{if client.text(text.to_string()).await.is_err(){break;}},
                    Ok(Message::Binary(bytes))=>{if client.binary(bytes).await.is_err(){break;}},
                    Ok(Message::Close(_))|Err(_)=>break,
                    _=>{},
                }
            }
        }
        session.controller.store(false, Ordering::SeqCst);
        let _ = client.close(None).await;
    });
    response
}

#[cfg(test)]
#[path = "realtime_api_tests.rs"]
mod tests;
