pub(crate) use crate::protocol_task::{run_plugin, run_with_failover};
use crate::ServerState;
use actix_web::HttpMessage;
use actix_web::{web, HttpRequest, HttpResponse};
use base64::prelude::{Engine as _, BASE64_STANDARD};
use futures_util::StreamExt;
use mediagen_core::TaskKind;
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub(crate) fn error(status: u16, code: &str, message: &str) -> HttpResponse {
    let kind = match status {
        401 => "authentication_error",
        403 => "permission_error",
        429 => "rate_limit_error",
        500..=599 => "server_error",
        _ => "invalid_request_error",
    };
    HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status)
            .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
    )
    .json(json!({"error":{"message":message,"type":kind,"param":null,"code":code}}))
}

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
    if let Err(message) = crate::protocol_request::validate_request(&body, endpoint) {
        return error(400, "invalid_request", &message);
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
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
    payload["model"] = json!(model.model_id);
    let capabilities = model
        .extra
        .get("capabilities")
        .cloned()
        .unwrap_or(Value::Null);
    if let Err(message) = crate::protocol_request::validate_capabilities(&payload, &capabilities) {
        return error(400, "unsupported_parameter", &message);
    }

    let local = request
        .extensions()
        .get::<crate::protocol_websocket::LocalContinuation>()
        .is_some_and(|local| payload["previous_response_id"].as_str() == Some(local.0.as_str()));
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
    if let Err(message) = crate::protocol_files::resolve(&state, &key, &mut payload).await {
        return error(400, "invalid_file_id", &message);
    }
    let store_response = endpoint == "responses" && body["store"] != false;
    let previous_response_id = body["previous_response_id"].clone();
    let metadata = body.get("metadata").cloned().unwrap_or(json!({}));
    let request_id = crate::request_identity::id(&request);
    let task_id = request_id.clone();
    let (mut account, lease) = match crate::protocol_task::reserve(
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
    ) {
        Ok(value) => value,
        Err(_) => return error(429, "account_unavailable", "没有可用账号或账号代理不可用"),
    };
    let streaming = body["stream"] == true && endpoint != "speech";
    let (sender, mut receiver) = mpsc::channel(8);
    let events = streaming.then_some(sender.clone());
    let task_state = state.clone();
    let task_request_id = request_id.clone();
    let is_audio = endpoint == "speech";
    let mut job = AbortJob(tokio::spawn(async move {
        let _lease = lease;
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
        if let Ok(value) = &mut result {
            if value["response"]["object"] == "response" {
                value["response"]["store"] = json!(store_response);
                value["response"]["metadata"] = metadata;
                value["response"]["previous_response_id"] = previous_response_id;
                if value["terminal_event"]["response"].is_object() {
                    value["terminal_event"]["response"] = value["response"].clone();
                }
            }
        }
        match &result {
            Ok(value) => {
                task_state.store.mark_account_usage_for(
                    &account.id,
                    if is_audio { "audio_speech" } else { "text" },
                );
                task_state
                    .store
                    .complete_task(&task_id, if store_response { json!({"task_id":task_id,"owner":crate::access_control::hash_secret(&key),"model_id":model.model_id,"input":stored_input,"response":value["response"]}) } else { json!({"status":"completed"}) });
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
                let (_, code, _) = public_failure(message);
                task_state.store.add_audit_log(
                    "protocol",
                    "error",
                    "生成请求失败".into(),
                    format!("request_id={task_request_id}; code={code}"),
                    Some(task_id.clone()),
                );
                task_state
                    .store
                    .fail_task(&task_id, "插件执行失败，请通过请求标识检查上游状态".into());
            }
        }
        if let Ok(value) = &result {
            if let Some(event) = value.get("terminal_event") {
                let _ = sender.send(event.clone()).await;
            }
        }
        if let Err(message) = &result {
            let (status, code, public) = public_failure(message);
            let _=sender.send(json!({"type":"error","status":status,"error":{"type":if status==400 {"invalid_request_error"} else {"server_error"},"code":code,"message":public,"param":null}})).await;
        }
        result
    }));
    if streaming {
        let first = match tokio::time::timeout(std::time::Duration::from_secs(180), receiver.recv())
            .await
        {
            Ok(Some(event)) => event,
            _ => return error(504, "upstream_timeout", "等待上游响应超时或流已关闭"),
        };
        if first["type"] == "error" {
            return HttpResponse::build(
                actix_web::http::StatusCode::from_u16(
                    first["status"].as_u64().unwrap_or(502) as u16
                )
                .unwrap_or(actix_web::http::StatusCode::BAD_GATEWAY),
            )
            .json(json!({"error":first["error"]}));
        }
        let first = Some(first);
        let encoder = crate::protocol_stream::Encoder::new(
            endpoint == "chat",
            model_id,
            body["stream_options"]["include_usage"] == true,
        );
        let stream = futures_util::stream::unfold(
            (receiver, encoder, job, first),
            |(mut receiver, mut encoder, job, mut first)| async move {
                loop {
                    if encoder.terminal {
                        return None;
                    }
                    let event = if let Some(event) = first.take() {
                        event
                    } else {
                        receiver.recv().await?
                    };
                    let frame = encoder.encode(&event);
                    if !frame.is_empty() {
                        return Some((
                            Ok::<_, actix_web::Error>(web::Bytes::from(frame)),
                            (receiver, encoder, job, first),
                        ));
                    }
                }
            },
        );
        return HttpResponse::Ok()
            .insert_header(("x-request-id", request_id))
            .insert_header(("cache-control", "no-cache"))
            .insert_header(("x-accel-buffering", "no"))
            .content_type("text/event-stream")
            .streaming(stream);
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

struct AbortJob(tokio::task::JoinHandle<Result<Value, String>>);
impl Drop for AbortJob {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(crate) fn public_failure(message: &str) -> (u16, &'static str, String) {
    if message == "task_cancelled" {
        return (409, "task_cancelled", "请求已取消".into());
    }
    if message.contains("invalid_input")
        || message.contains("missing_input")
        || message.contains("unsupported_")
    {
        (
            400,
            "unsupported_parameter",
            "当前模型不支持请求中的参数，请查询模型能力".into(),
        )
    } else if message.contains("429") || message.contains("quota_exhausted") {
        (
            429,
            "rate_limit_exceeded",
            "上游额度或速率限制，请稍后重试".into(),
        )
    } else if message.contains("LOGIN_EXPIRED") {
        (
            503,
            "account_authentication_failed",
            "上游账号需要重新登录".into(),
        )
    } else {
        (502, "upstream_error", "上游请求未成功完成".into())
    }
}
