#[path = "protocol_ingress.rs"]
mod ingress;
#[path = "protocol_response.rs"]
mod response;

pub(crate) use ingress::{chat, compact, dispatch, responses, speech};
pub(crate) use response::AbortJob;

pub(crate) use crate::protocol_task::{run_plugin, run_with_failover};
use crate::ServerState;
use actix_web::{web, HttpResponse};
use mediagen_core::TaskKind;
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub(crate) use crate::protocol_error::error;
use crate::protocol_error::error_kind;

pub(crate) async fn execute(
    state: web::Data<ServerState>,
    context: crate::request_identity::ProtocolRequest,
    body: Value,
    endpoint: &str,
    key: String,
    receipt_id: Option<uuid::Uuid>,
) -> HttpResponse {
    if let Err(message) = crate::protocol_request::validate_request(&body, endpoint) {
        return error(400, "invalid_request", &message);
    }
    if let Some(response) = crate::protocol_error::replay_failure(&state, &context.task_id, &body) {
        return response;
    }
    let model_id = body["model"].as_str().unwrap_or_default().to_string();
    crate::model_api::refresh_before_dispatch(&state, &model_id, &key).await;
    let Some((manifest, model, _)) = crate::model_api::resolve(&state, &model_id, &key) else {
        if state.plugin_loader.resolve_model(&model_id).is_some() {
            return error(
                403,
                "no_available_account_for_model",
                &format!("没有账号可以使用模型 {model_id}，当前已登录账号未提供该模型的权限"),
            );
        }
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
    payload.as_object_mut().unwrap().remove("callback_url");
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

    if kind == TaskKind::Text {
        if let Err(message) = crate::protocol_queue::check_model_accounts(
            &state, &manifest, &model_id, "text", &key, None,
        ) {
            let (status, code, message) = public_failure(&message);
            return error(status, code, &message);
        }
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
        options.remove("migrate_account");
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
    let mut complete_history = true;
    let session_lock = if let Some(id) = session_id.as_ref() {
        affinity_id = id.clone();
        let locks = crate::plugin_locks::PluginLocks::default();
        if let Err(message) = locks.acquire(&format!("conversation:{affinity_id}")).await {
            return error(409, "conversation_busy", &message);
        }
        match crate::conversation_affinity::read_session(&state, &affinity_id) {
            Ok(Some(saved)) => {
                if !state
                    .store
                    .task_by_id(&context.task_id)
                    .is_some_and(|task| task.status == "completed")
                {
                    complete_history = match crate::conversation_replay::restore(
                        &state,
                        &affinity_id,
                        &saved,
                        &mut payload,
                        body["previous_response_id"].is_string(),
                    ) {
                        Ok(complete) => complete,
                        Err(message) => {
                            return error(409, "conversation_context_unavailable", &message)
                        }
                    };
                }
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
    let migrating = match crate::conversation_replay::route(
        &state,
        &manifest,
        &key,
        &context.task_id,
        &mut payload,
        &mut preferred,
        complete_history,
    )
    .await
    {
        Ok(migrating) => migrating,
        Err(message) => {
            let (status, code, message) = public_failure(&message);
            return error(status, code, &message);
        }
    };
    let stored_input = crate::protocol_storage::input_items(&payload["input"]);
    let replay = (session_lock.is_some() && complete_history)
        .then(|| crate::conversation_replay::fresh_request(&payload));
    // 上游已保存的前文无需再次解析/上传附件；保留系统指令和本轮增量。
    crate::protocol_files::retain_current_input(&mut payload);
    let upload_use =
        match crate::upload_cleanup::register(&state, &context.task_id, &key, &payload).await {
            Ok(guard) => guard,
            Err(_) => {
                return if migrating {
                    error(
                        409,
                        "conversation_attachment_unavailable",
                        "旧会话附件已清理，无法完整迁移，请重新上传附件",
                    )
                } else {
                    error(400, "invalid_file_id", "文件不存在、已清理或无权访问")
                }
            }
        };
    if body["async"] == true && !context.queued {
        return crate::generation_queue::enqueue_text(state, key, body, endpoint, context).await;
    }
    let already_completed = state
        .store
        .task_by_id(&context.task_id)
        .is_some_and(|task| task.status == "completed");
    if !already_completed {
        if let Err(message) =
            crate::protocol_files::resolve(&state, &key, &mut payload, &capabilities).await
        {
            return if migrating {
                error(
                    409,
                    "conversation_attachment_unavailable",
                    "旧会话附件已清理或无法读取，无法完整迁移，请重新上传附件",
                )
            } else {
                error(400, "invalid_file_id", &message)
            };
        }
        if let Err(message) =
            crate::protocol_attachments::prepare(&state, &key, &mut payload, &capabilities).await
        {
            return error(400, "invalid_attachment", &message);
        }
        if let Err(message) =
            crate::protocol_request::validate_capabilities(&payload, &capabilities)
        {
            return error(400, "unsupported_parameter", &message);
        }
    }
    let store_response = endpoint == "responses" && body["store"] != false;
    let previous_response_id = body["previous_response_id"].clone();
    let metadata = body.get("metadata").cloned().unwrap_or(json!({}));
    let request_id = context.request_id;
    let task_id = context.task_id;
    let recoverable = receipt_id.is_some();
    if let Some(id) = receipt_id {
        match crate::request_receipts::reserve(&state, &key, &id.to_string(), &task_id).await {
            Ok(true) => {}
            Ok(false) => return error(409, "request_already_registered", "请查询原请求回执"),
            Err(_) => return error(503, "receipt_unavailable", "无法登记请求"),
        }
    }
    let session_base = if session_lock.is_some() {
        match crate::conversation_affinity::begin_session(&state, &affinity_id, &task_id).await {
            Ok(base) => base,
            Err(message) => {
                if recoverable {
                    crate::request_receipts::fail(&state, &task_id).await;
                }
                return error(409, "conversation_recovery_failed", &message);
            }
        }
    } else {
        None
    };
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
            if recoverable {
                crate::request_receipts::fail(&state, &task_id).await;
            }
            return crate::protocol_error::account_reservation_failure(&message);
        }
    };
    let streaming = body["stream"] == true && endpoint != "speech";
    if endpoint != "speech" {
        crate::conversation_affinity::remember(&state.store, &affinity_id, &account.id);
    }
    let (sender, receiver) = mpsc::channel(8);
    // HTTP 消费者断开后仍排空事件，让插件完成原调用并保存最终回执。
    let sender = if streaming && recoverable {
        let (events, mut drain) = mpsc::channel(8);
        let out = sender.clone();
        tokio::spawn(async move {
            while let Some(event) = drain.recv().await {
                let _ = out.send(event).await;
            }
        });
        events
    } else {
        sender
    };
    let events = streaming.then_some(sender.clone());
    let task_state = state.clone();
    let task_request_id = task_id.clone();
    let trace_request_id = request_id.clone();
    let task_endpoint = endpoint.to_owned();
    let is_audio = endpoint == "speech";
    let queued = context.queued;
    let is_chat = endpoint == "chat";
    let response_model_id = model_id.clone();
    let handle = tokio::spawn(async move {
        let _upload_use = upload_use;
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
            replay,
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
        if result
            .as_ref()
            .is_ok_and(|value| response::invalid_upstream_response(value, is_audio, streaming))
        {
            result = Err("invalid_upstream_response".into());
        }
        if let Ok(value) = &result {
            if _session_lock.is_some() && !already_completed {
                if let Err(message) = crate::conversation_replay::save(
                    &task_state,
                    &affinity_id,
                    &account.id,
                    &task_id,
                    session_base.as_deref(),
                    &json!(stored_input),
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
        if recoverable {
            if let Ok(value) = &result {
                let response = &value["response"];
                let status = if response["status"] == "completed" {
                    "completed"
                } else {
                    "failed"
                };
                if let Err(message) =
                    crate::request_receipts::finish(&task_state, &task_id, status, Some(response))
                        .await
                {
                    tracing::error!(%message, "恢复回执写入失败");
                }
            } else {
                crate::request_receipts::fail(&task_state, &task_id).await;
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
                    .complete_task(&task_id, if store_response { json!({"task_id":task_id,"owner":crate::access_control::hash_secret(&key),"model_id":model.model_id,"input":stored_input,"conversation_session_id":session_id,"response":value["response"]}) } else { json!({"status":"completed","conversation_session_id":session_id}) });
            }
            Err(message) if message == "task_cancelled" => {}
            Err(message) => {
                let context = crate::media_execution::MediaFlowContext {
                    manifest: &manifest,
                    model: &model,
                    account: account.clone(),
                    task_id: task_id.clone(),
                };
                let decision = crate::media_execution::classify_and_apply_failure(
                    &task_state,
                    &context,
                    message,
                );
                crate::protocol_error::record_failure(
                    &task_state,
                    &context,
                    &trace_request_id,
                    &task_endpoint,
                    &decision,
                    &key,
                );
                let (_, code, public) = public_failure(message);
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
    });
    let job = AbortJob {
        handle,
        detach: recoverable,
    };
    response::respond(
        job,
        receiver,
        streaming,
        endpoint,
        model_id,
        body["stream_options"]["include_usage"] == true,
        request_id,
    )
    .await
}

pub(crate) use crate::protocol_error::public_failure;
