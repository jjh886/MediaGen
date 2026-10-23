pub(crate) use crate::protocol_task::{run_plugin, run_with_failover};
use crate::ServerState;
use actix_web::{web, HttpRequest, HttpResponse};
use base64::prelude::{Engine as _, BASE64_STANDARD};
use futures_util::StreamExt;
use mediagen_core::TaskKind;
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub(crate) use crate::protocol_error::error;
use crate::protocol_error::error_kind;

pub(crate) async fn responses(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    read_and_dispatch(state, request, payload, "responses").await
}
pub(crate) async fn chat(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    read_and_dispatch(state, request, payload, "chat").await
}
pub(crate) async fn speech(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    read_and_dispatch(state, request, payload, "speech").await
}
pub(crate) async fn compact(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    read_and_dispatch(state, request, payload, "compact").await
}

async fn read_and_dispatch(
    state: web::Data<ServerState>,
    request: HttpRequest,
    mut payload: web::Payload,
    endpoint: &str,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    if !request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        })
    {
        return error(415, "unsupported_media_type", "此接口要求 application/json");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = payload.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => return error(400, "invalid_body", "读取请求体失败"),
        };
        if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
            return error(413, "body_too_large", "请求体超过 16 MiB 限制");
        }
        bytes.extend_from_slice(&chunk);
    }
    dispatch(state, request, web::Bytes::from(bytes), endpoint).await
}

pub(crate) async fn dispatch(
    state: web::Data<ServerState>,
    request: HttpRequest,
    bytes: web::Bytes,
    endpoint: &str,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let body: Value = match serde_json::from_slice(&bytes) {
        Ok(body) => body,
        Err(_) => return error(400, "invalid_json", "请求体不是有效 JSON"),
    };
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let context = crate::request_identity::ProtocolRequest::from_http(&request, &key, &body);
    execute(state, context, body, endpoint, key).await
}

pub(crate) async fn execute(
    state: web::Data<ServerState>,
    context: crate::request_identity::ProtocolRequest,
    body: Value,
    endpoint: &str,
    key: String,
) -> HttpResponse {
    if let Err(message) = crate::protocol_request::validate_request(&body, endpoint) {
        return error(400, "invalid_request", &message);
    }
    let model_id = body["model"].as_str().unwrap_or_default().to_string();
    let Some((manifest, model, _)) = crate::model_api::resolve(&state, &model_id, &key) else {
        return error(404, "model_not_found", "模型不存在，请先获取模型目录");
    };
    let (kind, flow) = if endpoint == "speech" {
        (TaskKind::AudioSpeech, "generate_audio")
    } else if endpoint == "compact" {
        (TaskKind::Text, "compact_text")
    } else {
        (TaskKind::Text, "generate_text")
    };
    if model.task_kind != kind || !manifest.flows.contains_key(flow) {
        return error(400, "unsupported_endpoint", "模型不支持当前接口");
    }
    let mut payload = if endpoint == "chat" {
        match crate::protocol_request::chat_to_responses(&body) {
            Ok(value) => value,
            Err(message) => return error(400, "invalid_request", &message),
        }
    } else {
        body.clone()
    };
    payload.as_object_mut().unwrap().remove("async");
    payload["model"] = json!(model.model_id);
    let capabilities = model
        .extra
        .get("capabilities")
        .cloned()
        .unwrap_or(Value::Null);

    let parameter_payload = Value::Object(
        payload
            .as_object()
            .ok_or(())
            .map(|fields| {
                fields
                    .iter()
                    .filter(|(key, _)| key.as_str() != "input")
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    );
    if let Err(message) =
        crate::protocol_request::validate_capabilities(&parameter_payload, &capabilities)
    {
        return error(400, "unsupported_parameter", &message);
    }

    if body["async"] == true && !context.queued {
        return crate::generation_queue::enqueue_text(state, key, body, endpoint, context).await;
    }
    let local = context
        .local_previous
        .as_deref()
        .is_some_and(|id| payload["previous_response_id"].as_str() == Some(id));
    if local {
        payload
            .as_object_mut()
            .unwrap()
            .remove("previous_response_id");
    }
    if let Err(message) = crate::protocol_storage::previous_input(&state, &key, &mut payload) {
        return error(400, "invalid_previous_response", &message);
    }
    let stored_input = crate::protocol_storage::input_items(&payload["input"]);
    if payload
        .get("options")
        .is_some_and(|value| !value.is_object())
    {
        return error(400, "invalid_options", "options 必须是对象");
    }
    if let Some(options) = payload.get_mut("options").and_then(Value::as_object_mut) {
        options.remove("platform_session");
        options.remove("session_input_start");
        options.remove("session_key");
    }
    let (mut affinity_id, mut preferred) = crate::conversation_affinity::identify(
        &state,
        context.connection_id.as_deref(),
        &key,
        &manifest.platform_id,
        &body,
        &payload,
    );
    let session_id = crate::conversation_affinity::session_identity(
        &state,
        context.connection_id.as_deref(),
        &key,
        &manifest.platform_id,
        &body,
        endpoint,
        &context.task_id,
    );
    let session_lock = if let Some(id) = session_id.as_ref() {
        affinity_id = id.clone();
        let locks = crate::plugin_locks::PluginLocks::default();
        if let Err(message) = locks.acquire(&format!("conversation:{affinity_id}")).await {
            return error(409, "conversation_busy", &message);
        }
        match crate::conversation_affinity::read_session(&state, &affinity_id) {
            Ok(Some(saved)) => {
                preferred = crate::conversation_affinity::apply_cursor(&mut payload, &saved);
            }
            Ok(None) => {}
            Err(message) => return error(409, "conversation_recovery_failed", &message),
        }
        if !payload["options"].is_object() {
            payload["options"] = json!({});
        }
        payload["options"]["session_key"] = json!(affinity_id);
        Some(locks)
    } else {
        None
    };
    if let Err(message) = crate::protocol_files::resolve(&state, &key, &mut payload).await {
        return error(400, "invalid_file_id", &message);
    }
    if let Err(message) =
        crate::protocol_attachments::prepare(&state, &key, &mut payload, &capabilities).await
    {
        return error(400, "invalid_attachment", &message);
    }
    if let Err(message) = crate::protocol_request::validate_capabilities(&payload, &capabilities) {
        return error(400, "unsupported_parameter", &message);
    }
    let store_response = endpoint == "responses" && body["store"] != false;
    let previous_response_id = body["previous_response_id"].clone();
    let metadata = body.get("metadata").cloned().unwrap_or(json!({}));
    let request_id = context.request_id;
    let task_id = context.task_id;
    let session_base = if session_lock.is_some() {
        match crate::conversation_affinity::begin_session(&state, &affinity_id, &task_id).await {
            Ok(base) => base,
            Err(message) => return error(409, "conversation_recovery_failed", &message),
        }
    } else {
        None
    };
    let already_completed = state
        .store
        .task_by_id(&task_id)
        .is_some_and(|task| task.status == "completed");
    let (mut account, lease) = match crate::protocol_task::reserve_affinity_wait(
        &state,
        &manifest,
        &model_id,
        if endpoint == "speech" {
            "audio_speech"
        } else {
            "text"
        },
        &key,
        &task_id,
        Some(&payload),
        preferred.as_deref(),
    )
    .await
    {
        Ok(value) => value,
        Err(message) => {
            let code = if message.contains("等待") {
                "account_queue_timeout"
            } else {
                "account_unavailable"
            };
            let mut response = error(429, code, &message);
            response.headers_mut().insert(
                actix_web::http::header::RETRY_AFTER,
                actix_web::http::header::HeaderValue::from_static("30"),
            );
            return response;
        }
    };
    let streaming = body["stream"] == true && endpoint != "speech";
    if endpoint != "speech" {
        crate::conversation_affinity::remember(&state.store, &affinity_id, &account.id);
    }
    let (sender, receiver) = mpsc::channel(8);
    let events = streaming.then_some(sender.clone());
    let task_state = state.clone();
    let task_request_id = task_id.clone();
    let is_audio = endpoint == "speech";
    let queued = context.queued;
    let is_chat = endpoint == "chat";
    let response_model_id = model_id.clone();
    let mut job = AbortJob(tokio::spawn(async move {
        let _lease = lease;
        let _session_lock = session_lock;
        let execution = run_with_failover(
            &task_state,
            &manifest,
            &mut account,
            flow,
            payload,
            events,
            &task_request_id,
            capabilities,
            &key,
        );
        let cancelled = async {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if task_state
                    .store
                    .task_by_id(&task_id)
                    .is_some_and(|task| task.status == "cancelled")
                {
                    break;
                }
            }
        };
        let mut result =
            tokio::select! {result=execution=>result,_=cancelled=>Err("task_cancelled".into())};
        if result.as_ref().is_ok_and(|value| {
            if is_audio {
                value["audio_base64"]
                    .as_str()
                    .is_none_or(|audio| BASE64_STANDARD.decode(audio).is_err())
            } else {
                value["response"]["id"].as_str().is_none()
                    || !value["response"]["output"].is_array()
                    || (streaming && value.get("terminal_event").is_none())
            }
        }) {
            result = Err("invalid_upstream_response".into());
        }
        if let Ok(value) = &result {
            if _session_lock.is_some() && !already_completed {
                if let Err(message) = crate::conversation_affinity::write_session(
                    &task_state,
                    &affinity_id,
                    &account.id,
                    &task_id,
                    session_base.as_deref(),
                    &value["response"],
                )
                .await
                {
                    result = Err(message);
                }
            }
        }
        if let Ok(value) = &mut result {
            crate::conversation_affinity::redact_cursor(value);
            if value["response"]["object"] == "response" {
                value["response"]["store"] = json!(store_response);
                value["response"]["metadata"] =
                    crate::protocol_storage::response_metadata(&value["response"], &metadata);
                value["response"]["previous_response_id"] = previous_response_id;
                if value["terminal_event"]["response"].is_object() {
                    value["terminal_event"]["response"] = value["response"].clone();
                }
            }
        }
        if queued {
            if let Ok(value) = &result {
                let response = if is_chat {
                    crate::protocol_request::chat_response(&value["response"], &response_model_id)
                } else {
                    value["response"].clone()
                };
                if let Err(message) =
                    crate::generation_queue::result::save(&task_state, &task_id, &response).await
                {
                    result = Err(message);
                }
            }
        }
        match &result {
            Ok(value) => {
                if !is_audio {
                    crate::conversation_affinity::remember(
                        &task_state.store,
                        &affinity_id,
                        &account.id,
                    );
                }
                if !already_completed {
                    task_state.store.mark_account_usage_for(
                        &account.id,
                        if is_audio { "audio_speech" } else { "text" },
                    );
                }
                task_state
                    .store
                    .complete_task(&task_id, if store_response { json!({"task_id":task_id,"owner":crate::access_control::hash_secret(&key),"model_id":model.model_id,"input":stored_input,"conversation_session_id":session_id,"response":value["response"]}) } else { json!({"status":"completed"}) });
            }
            Err(message) if message == "task_cancelled" => {}
            Err(message) => {
                let context = crate::media_execution::MediaFlowContext {
                    manifest: &manifest,
                    model: &model,
                    account: account.clone(),
                    task_id: task_id.clone(),
                };
                crate::media_execution::classify_and_apply_failure(&task_state, &context, message);
                let (_, code, public) = public_failure(message);
                task_state.store.add_audit_log(
                    "protocol",
                    "error",
                    "生成请求失败".into(),
                    format!("request_id={task_request_id}; code={code}"),
                    Some(task_id.clone()),
                );
                task_state
                    .store
                    .fail_task(&task_id, format!("{code}: {public}"));
            }
        }
        if let Ok(value) = &result {
            if let Some(event) = value.get("terminal_event") {
                let _ = sender.send(event.clone()).await;
            }
        }
        if let Err(message) = &result {
            let (status, code, public) = public_failure(message);
            let _=sender.send(json!({"type":"error","status":status,"error":{"type":error_kind(status),"code":code,"message":public,"param":null}})).await;
        }
        result
    }));
    if streaming {
        return crate::protocol_stream::response(
            receiver,
            job,
            endpoint,
            model_id,
            body["stream_options"]["include_usage"] == true,
            request_id,
        )
        .await;
    }
    drop(receiver);
    match (&mut job.0).await {
        Ok(Ok(result)) if endpoint == "speech" => {
            let audio = result["audio_base64"]
                .as_str()
                .and_then(|value| BASE64_STANDARD.decode(value).ok());
            match audio.filter(|value| !value.is_empty()) {
                Some(audio) => HttpResponse::Ok()
                    .insert_header(("x-request-id", request_id))
                    .content_type(
                        result["mime_type"]
                            .as_str()
                            .unwrap_or("application/octet-stream"),
                    )
                    .body(audio),
                None => error(502, "invalid_audio", "上游没有返回有效音频"),
            }
        }
        Ok(Ok(result)) => {
            let response = if endpoint == "chat" {
                crate::protocol_request::chat_response(&result["response"], &model_id)
            } else {
                result["response"].clone()
            };
            HttpResponse::Ok()
                .insert_header(("x-request-id", request_id))
                .json(response)
        }
        Ok(Err(message)) => {
            let (status, code, public) = public_failure(&message);
            error(status, code, &public)
        }
        Err(_) => error(500, "execution_failed", "请求执行中断"),
    }
}

pub(crate) struct AbortJob(tokio::task::JoinHandle<Result<Value, String>>);
impl Drop for AbortJob {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(crate) use crate::protocol_error::public_failure;
