use actix_web::{web, HttpRequest, HttpResponse, Responder};
use mediagen_core::TaskKind;
use serde_json::Value;

use crate::access_control::hash_secret;
use crate::image_api_response::{new_image_task, openai_error};
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
    let request_id = crate::request_identity::id(&request);
    body.apply_endpoint_defaults(endpoint);
    handle_image_request(state, body, endpoint, caller_key, request_id).await
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
    let request_id = crate::request_identity::id(&request);
    let mut body = match parse_image_request(request, payload).await {
        Ok(body) => body,
        Err((status, code, message)) => return openai_error(status, code, &message),
    };
    body.apply_endpoint_defaults(endpoint);
    handle_image_request(state, body, endpoint, caller_key, request_id).await
}

async fn handle_image_request(
    state: web::Data<ServerState>,
    mut body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
    caller_key: Option<String>,
    request_id: String,
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
    if let Some((manifest, model)) = state.plugin_loader.resolve_model(&body.model) {
        if let Err(message) = crate::image_parameters::validate(
            &body,
            endpoint,
            &crate::image_parameters::model_limits(
                manifest
                    .extra
                    .get("image_capabilities")
                    .unwrap_or(&Value::Null),
                model.extra.get("capabilities").unwrap_or(&Value::Null),
            ),
        ) {
            return openai_error(400, "unsupported_parameter", &message);
        }
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
    if body.stream == Some(true) {
        if let Err((status, code, message)) = resolve_image_flow_context_with_task_id(
            &state,
            &body,
            caller_key.as_deref(),
            request_id.clone(),
        ) {
            return openai_error(status, code, &message);
        }
        return crate::image_stream::response(
            endpoint,
            run_image_flow(state, body, endpoint, caller_key, request_id),
        );
    }
    match run_image_flow(state, body, endpoint, caller_key, request_id).await {
        Ok(response_body) => HttpResponse::Ok().json(response_body),
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
    let _execution_guard = ImageExecutionGuard {
        store: state.store.clone(),
        task_id: context.task_id.clone(),
    };
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

struct ImageExecutionGuard {
    store: std::sync::Arc<crate::store::AppStore>,
    task_id: String,
}

impl Drop for ImageExecutionGuard {
    fn drop(&mut self) {
        // 持久队列由 worker 处理重试；执行器退出不能伪装成用户取消。
        if self.store.generation_job(&self.task_id, None).is_some() {
            return;
        }
        if self
            .store
            .task_by_id(&self.task_id)
            .is_some_and(|task| matches!(task.status.as_str(), "queued" | "processing"))
        {
            self.store.cancel_task(&self.task_id);
        }
    }
}

async fn run_image_flow(
    state: web::Data<ServerState>,
    body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
    caller_key: Option<String>,
    request_id: String,
) -> Result<Value, (u16, &'static str, String)> {
    let settings = state.store.settings();
    let reservation = state
        .image_task_queue
        .reserve(&settings)
        .map_err(|message| (429, "queue_full", message))?;
    let context = state.store.with_account_dispatch(|| {
        let context = resolve_image_flow_context_with_task_id(
            &state,
            &body,
            caller_key.as_deref(),
            request_id.clone(),
        )?;
        state.store.insert_task(new_image_task(
            &context.task_id,
            context.manifest,
            &context.model.model_id,
            &body,
            &context.account.id,
        ));
        Ok(context)
    })?;
    let _execution_guard = ImageExecutionGuard {
        store: state.store.clone(),
        task_id: context.task_id.clone(),
    };
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
                return complete_attempt_success(state, &context, &body, result).await;
            }
            Err(message) => {
                if message.contains("invalid_input:") {
                    state
                        .store
                        .fail_task(&context.task_id, "输入参数或参考素材无效".into());
                    return Err((
                        400,
                        "invalid_request_error",
                        "输入参数或参考素材无效".into(),
                    ));
                }
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
        prepare_flow_request(body, context.model, endpoint, &context.account.proxy_url).await?;
    flow_request.task_id = Some(context.task_id.clone());
    let result = execute_image_flow(state, context.manifest, &context.account, flow_request).await;
    cleanup_temp_files(&ref_image_locals).await;
    result
}

async fn complete_attempt_success(
    state: &web::Data<ServerState>,
    context: &MediaFlowContext<'_>,
    body: &OpenAiImageRequest,
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
    complete_image_task(
        state,
        &context.task_id,
        &context.account.proxy_url,
        body,
        result,
    )
    .await
}

fn resolve_image_flow_context_with_task_id<'a>(
    state: &'a web::Data<ServerState>,
    body: &OpenAiImageRequest,
    caller_key: Option<&str>,
    task_id: String,
) -> Result<MediaFlowContext<'a>, (u16, &'static str, String)> {
    resolve_media_flow_context(state, &body.model, TaskKind::Image, caller_key, task_id)
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn dropping_queued_execution_preserves_retry_and_original_account() {
        let root = std::env::temp_dir().join(format!("image-restart-{}", uuid::Uuid::new_v4()));
        let store = std::sync::Arc::new(crate::store::AppStore::load(root.join("state.json")));
        let body =
            serde_json::from_value(serde_json::json!({"model":"model","prompt":"原请求"})).unwrap();
        let (task, job) = crate::generation_queue::queued_image_records(
            "task".into(),
            "plugin".into(),
            &body,
            "owner",
            "encrypted".into(),
            chrono::Utc::now().to_rfc3339(),
        )
        .unwrap();
        store.create_generation_job(task, job).unwrap();
        let now = chrono::Utc::now();
        store
            .claim_next_generation_job(now, now + chrono::Duration::hours(6))
            .unwrap();
        store.set_task_account("task", "original");
        store.start_task("task");
        drop(ImageExecutionGuard {
            store: store.clone(),
            task_id: "task".into(),
        });
        assert_eq!(store.task_by_id("task").unwrap().status, "processing");
        drop(store);
        let restored = crate::store::AppStore::load(root.join("state.json"));
        restored.recover_leased_generation_jobs(now);
        assert_eq!(
            restored.task_by_id("task").unwrap().account_id.as_deref(),
            Some("original")
        );
        assert_eq!(
            restored.generation_job("task", None).unwrap().state,
            crate::store::GenerationJobState::RetryWait
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
