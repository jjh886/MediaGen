use actix_web::{web, HttpRequest, HttpResponse, Responder};
use mediagen_core::TaskKind;
use serde_json::Value;
use uuid::Uuid;

use crate::access_control::hash_secret;
use crate::image_api_response::{new_image_task, openai_error, openai_image_stream};
use crate::image_assets::cleanup_temp_files;
use crate::image_flow_execution::{execute_image_flow, prepare_flow_request};
use crate::image_multipart::parse_image_request;
use crate::image_request::{ImageEndpoint, OpenAiImageRequest};
use crate::image_task_completion::complete_image_task;
use crate::media_execution::{
    ensure_task_not_cancelled, handle_attempt_failure, resolve_media_flow_context, MediaFlowContext,
};
use crate::routes::{api_key_allowed, api_key_from_request};
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
    mut context: MediaFlowContext<'_>,
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
                if let Some(response) = handle_attempt_failure(
                    state,
                    &mut context,
                    &message,
                    "图片",
                    &mut same_account_failures,
                    caller_key.as_deref(),
                    &mut excluded,
                )
                .await?
                {
                    return response;
                }
                continue;
            }
        }
    }
}

async fn run_single_image_attempt(
    state: &web::Data<ServerState>,
    body: &OpenAiImageRequest,
    endpoint: ImageEndpoint,
    context: &MediaFlowContext<'_>,
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
    context: &MediaFlowContext<'_>,
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

fn resolve_image_flow_context<'a>(
    state: &'a web::Data<ServerState>,
    body: &OpenAiImageRequest,
    caller_key: Option<&str>,
) -> Result<MediaFlowContext<'a>, (u16, &'static str, String)> {
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
) -> Result<MediaFlowContext<'a>, (u16, &'static str, String)> {
    resolve_media_flow_context(state, &body.model, TaskKind::Image, caller_key, task_id)
}
