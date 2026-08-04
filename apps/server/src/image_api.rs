use actix_web::{web, HttpRequest, HttpResponse, Responder};
use mediagen_core::{ModelDef, PluginManifest};
use serde_json::Value;
use uuid::Uuid;

use crate::access_control::hash_secret;
use crate::image_api_response::{new_image_task, openai_error, openai_image_stream};
use crate::image_assets::cleanup_temp_files;
use crate::image_flow_execution::{execute_image_flow, prepare_flow_request};
use crate::image_multipart::parse_image_request;
use crate::image_request::{ImageEndpoint, OpenAiImageRequest};
use crate::image_task_completion::{complete_image_task, create_verification_session};
use crate::routes::{api_key_allowed, api_key_from_request};
use crate::store::AccountRecord;
use crate::ServerState;

pub async fn generate_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    body: web::Json<OpenAiImageRequest>,
) -> impl Responder {
    handle_json_image(state, request, body.into_inner(), ImageEndpoint::Generation).await
}

pub async fn edit_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> impl Responder {
    handle_payload_image(state, request, payload, ImageEndpoint::Edit).await
}

pub async fn variation_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> impl Responder {
    handle_payload_image(state, request, payload, ImageEndpoint::Variation).await
}

async fn handle_json_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    mut body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
) -> HttpResponse {
    if !api_key_allowed(&state, &request) {
        return openai_error(401, "authentication_error", "invalid api key");
    }
    let caller_key = api_key_from_request(&request);
    body.apply_endpoint_defaults(endpoint);
    handle_image_request(state, body, endpoint, caller_key).await
}

async fn handle_payload_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
    endpoint: ImageEndpoint,
) -> HttpResponse {
    if !api_key_allowed(&state, &request) {
        return openai_error(401, "authentication_error", "invalid api key");
    }
    let caller_key = api_key_from_request(&request);
    let mut body = match parse_image_request(request, payload).await {
        Ok(body) => body,
        Err((status, code, message)) => return openai_error(status, code, &message),
    };
    body.apply_endpoint_defaults(endpoint);
    handle_image_request(state, body, endpoint, caller_key).await
}

async fn handle_image_request(
    state: web::Data<ServerState>,
    mut body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
    caller_key: Option<String>,
) -> HttpResponse {
    let owner_hash = caller_key.as_deref().map(hash_secret).unwrap_or_default();
    if let Err((status, code, message)) =
        crate::image_file_refs::resolve_openai_file_refs(&state.file_root, &mut body, &owner_hash)
            .await
    {
        return openai_error(status, code, &message);
    }
    if let Err((status, code, message)) = body.validate(endpoint) {
        return openai_error(status, code, &message);
    }
    if body.async_generation {
        if endpoint != ImageEndpoint::Generation {
            return openai_error(
                400,
                "unsupported_parameter",
                "async 排队模式目前只支持图片生成接口",
            );
        }
        let Some(caller_key) = caller_key else {
            return openai_error(401, "authentication_error", "invalid api key");
        };
        return crate::generation_queue::enqueue_image(state, caller_key, body).await;
    }
    let stream = body.stream == Some(true);
    match run_image_flow(state, body, endpoint, caller_key).await {
        Ok(response_body) => {
            if stream {
                openai_image_stream(response_body, endpoint)
            } else {
                HttpResponse::Ok().json(response_body)
            }
        }
        Err((status, code, message)) => openai_error(status, code, &message),
    }
}

pub(crate) async fn run_queued_image_flow(
    state: web::Data<ServerState>,
    body: OpenAiImageRequest,
    caller_key: String,
    task_id: &str,
) -> Result<Value, (u16, &'static str, String)> {
    ensure_task_not_cancelled(&state, task_id)?;
    let context = state.store.with_account_dispatch(|| {
        let context = resolve_image_flow_context_with_task_id(
            &state,
            &body,
            Some(&caller_key),
            task_id.into(),
        )?;
        state.store.set_task_account(task_id, &context.account.id);
        state.store.start_task(task_id);
        Ok(context)
    })?;
    state
        .store
        .add_task_event(task_id, "info", "任务开始执行".to_string());
    run_image_attempts(
        &state,
        body,
        ImageEndpoint::Generation,
        Some(caller_key),
        context,
    )
    .await
}

async fn run_image_flow(
    state: web::Data<ServerState>,
    body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
    caller_key: Option<String>,
) -> Result<Value, (u16, &'static str, String)> {
    let settings = state.store.settings();
    let reservation = state
        .image_task_queue
        .reserve(&settings)
        .map_err(|message| (429, "queue_full", message))?;
    let context = state.store.with_account_dispatch(|| {
        let context = resolve_image_flow_context(&state, &body, caller_key.as_deref())?;
        state.store.insert_task(new_image_task(
            &context.task_id,
            context.manifest,
            &context.model.model_id,
            &body,
            &context.account.id,
        ));
        Ok(context)
    })?;
    state
        .store
        .add_task_event(&context.task_id, "info", "任务进入队列".to_string());
    let _permit = reservation.activate(&settings).await;
    state.store.start_task(&context.task_id);
    state
        .store
        .add_task_event(&context.task_id, "info", "任务开始执行".to_string());
    run_image_attempts(&state, body, endpoint, caller_key, context).await
}

async fn run_image_attempts(
    state: &web::Data<ServerState>,
    body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
    caller_key: Option<String>,
    mut context: ImageFlowContext<'_>,
) -> Result<Value, (u16, &'static str, String)> {
    let mut excluded = Vec::<String>::new();
    let mut same_account_failures = 0u32;
    loop {
        ensure_task_not_cancelled(state, &context.task_id)?;
        state
            .store
            .set_task_account(&context.task_id, &context.account.id);
        match run_single_image_attempt(state, &body, endpoint, &context).await {
            Ok(result) => {
                ensure_task_not_cancelled(state, &context.task_id)?;
                return complete_attempt_success(state, &context, result).await;
            }
            Err(message) => {
                let decision = apply_failure_strategy(state, &context, &message);
                if decision.verification_required() {
                    return create_verification_session(
                        state,
                        context.manifest,
                        &context.account,
                        &context.task_id,
                        decision.original_message,
                    )
                    .await;
                }
                record_attempt_failure(state, &context, &decision);
                same_account_failures += 1;
                if decision.should_retry_same_account(same_account_failures) {
                    continue;
                }
                if decision.should_cooldown_after_retry_exhausted() {
                    state.store.mark_account_state_for_seconds(
                        &context.account.id,
                        "cooldown",
                        decision.threshold_cooldown_seconds,
                    );
                }
                if decision.should_failover() {
                    excluded.push(context.account.id.clone());
                    if let Some(next) =
                        next_failover_context(state, &context, caller_key.as_deref(), &excluded)?
                    {
                        context = next;
                        same_account_failures = 0;
                        continue;
                    }
                }
                return fail_image_task(state, &context, &decision);
            }
        }
    }
}

async fn run_single_image_attempt(
    state: &web::Data<ServerState>,
    body: &OpenAiImageRequest,
    endpoint: ImageEndpoint,
    context: &ImageFlowContext<'_>,
) -> Result<Value, String> {
    let (mut flow_request, ref_image_locals) =
        prepare_flow_request(body, context.model, endpoint, &context.account.proxy_url).await;
    flow_request.task_id = Some(context.task_id.clone());
    let result = execute_image_flow(state, context.manifest, &context.account, flow_request).await;
    cleanup_temp_files(&ref_image_locals).await;
    result
}

async fn complete_attempt_success(
    state: &web::Data<ServerState>,
    context: &ImageFlowContext<'_>,
    result: Value,
) -> Result<Value, (u16, &'static str, String)> {
    ensure_task_not_cancelled(state, &context.task_id)?;
    state.store.mark_account_usage(&context.account.id);
    state.store.add_audit_log(
        "engine",
        "info",
        "图片任务完成".to_string(),
        format!(
            "task={}, platform={}",
            context.task_id, context.manifest.platform_id
        ),
        Some(context.task_id.clone()),
    );
    complete_image_task(state, &context.task_id, &context.account.proxy_url, result).await
}

fn apply_failure_strategy(
    state: &web::Data<ServerState>,
    context: &ImageFlowContext<'_>,
    message: &str,
) -> crate::failure_strategy::FailureDecision {
    let decision = if let Some(message) = message.strip_prefix("needs_verification:") {
        crate::failure_strategy::FailureDecision {
            category: "verification_required".to_string(),
            retry_action: "failover".to_string(),
            account_state: Some("verification_required".to_string()),
            usage_consumed: false,
            same_error_threshold: 1,
            threshold_cooldown_seconds: 0,
            cooldown_from_message: false,
            cooldown_seconds: 0,
            original_message: message.to_string(),
        }
    } else {
        crate::failure_strategy::classify_failure(context.manifest, message)
    };
    apply_account_state_for_failure(state, context, &decision);
    decision
}

fn apply_account_state_for_failure(
    state: &web::Data<ServerState>,
    context: &ImageFlowContext<'_>,
    decision: &crate::failure_strategy::FailureDecision,
) {
    if let Some(account_state) = decision.account_state.as_deref() {
        if account_state == "cooldown" {
            let fallback = i64::from(state.store.settings().cooldown_default_seconds);
            state.store.mark_account_state_for_seconds(
                &context.account.id,
                account_state,
                decision.cooldown_seconds(&decision.original_message, fallback),
            );
        } else {
            state
                .store
                .mark_account_state(&context.account.id, account_state);
        }
    }
}

fn record_attempt_failure(
    state: &web::Data<ServerState>,
    context: &ImageFlowContext<'_>,
    decision: &crate::failure_strategy::FailureDecision,
) {
    if decision.usage_consumed {
        state.store.mark_account_usage(&context.account.id);
    }
    let message = decision.formatted_message();
    state
        .store
        .add_task_event(&context.task_id, "error", message.clone());
    state.store.add_audit_log(
        "engine",
        "error",
        "图片任务失败".to_string(),
        message,
        Some(context.task_id.clone()),
    );
}

fn next_failover_context<'a>(
    state: &'a web::Data<ServerState>,
    current: &ImageFlowContext<'a>,
    caller_key: Option<&str>,
    excluded: &[String],
) -> Result<Option<ImageFlowContext<'a>>, (u16, &'static str, String)> {
    let selected = state.store.with_account_dispatch(|| {
        let account = state.store.first_available_account_for_key_excluding(
            &current.manifest.platform_id,
            caller_key,
            excluded,
        )?;
        let account = state.store.account_with_required_runtime_proxy(account);
        if let Ok(account) = &account {
            state.store.set_task_account(&current.task_id, &account.id);
        }
        Some(account)
    });
    let Some(account) = selected else {
        return Ok(None);
    };
    let account = account.map_err(|message| (409, "proxy_unavailable", message))?;
    state.store.add_task_event(
        &current.task_id,
        "info",
        format!("切换账号继续执行: {}", account.id),
    );
    Ok(Some(ImageFlowContext {
        manifest: current.manifest,
        model: current.model,
        account,
        task_id: current.task_id.clone(),
    }))
}

fn fail_image_task(
    state: &web::Data<ServerState>,
    context: &ImageFlowContext<'_>,
    decision: &crate::failure_strategy::FailureDecision,
) -> Result<Value, (u16, &'static str, String)> {
    ensure_task_not_cancelled(state, &context.task_id)?;
    let message = decision.formatted_message();
    state.store.fail_task(&context.task_id, message.clone());
    let (status, code) = decision.response_error();
    Err((status, code, message))
}

struct ImageFlowContext<'a> {
    manifest: &'a PluginManifest,
    model: &'a ModelDef,
    account: AccountRecord,
    task_id: String,
}

fn resolve_image_flow_context<'a>(
    state: &'a web::Data<ServerState>,
    body: &OpenAiImageRequest,
    caller_key: Option<&str>,
) -> Result<ImageFlowContext<'a>, (u16, &'static str, String)> {
    resolve_image_flow_context_with_task_id(
        state,
        body,
        caller_key,
        format!("task-{}", Uuid::new_v4()),
    )
}

fn resolve_image_flow_context_with_task_id<'a>(
    state: &'a web::Data<ServerState>,
    body: &OpenAiImageRequest,
    caller_key: Option<&str>,
    task_id: String,
) -> Result<ImageFlowContext<'a>, (u16, &'static str, String)> {
    let (manifest, model) = state
        .plugin_loader
        .resolve_model(&body.model)
        .ok_or_else(|| {
            (
                404,
                "model_not_found",
                format!("模型 {} 不存在", body.model),
            )
        })?;
    let account = state
        .store
        .first_available_account_for_key(&manifest.platform_id, caller_key)
        .map(|account| state.store.account_with_required_runtime_proxy(account))
        .transpose()
        .map_err(|message| (409, "proxy_unavailable", message))?
        .ok_or_else(|| {
            (
                409,
                "account_unavailable",
                format!("{} 没有可用账号，请先添加账号。", manifest.display_name),
            )
        })?;
    Ok(ImageFlowContext {
        manifest,
        model,
        account,
        task_id,
    })
}

fn ensure_task_not_cancelled(
    state: &web::Data<ServerState>,
    task_id: &str,
) -> Result<(), (u16, &'static str, String)> {
    if state
        .store
        .task_by_id(task_id)
        .is_some_and(|task| task.status == "cancelled")
    {
        Err((409, "task_cancelled", "用户取消".to_string()))
    } else {
        Ok(())
    }
}
