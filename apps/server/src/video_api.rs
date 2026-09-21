use actix_web::{web, HttpRequest, HttpResponse, Responder};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::access_control::hash_secret;
use crate::image_api_response::openai_error;
use crate::image_assets::cleanup_temp_files;
use crate::media_execution::{
    ensure_task_not_cancelled, handle_attempt_failure, ApiError, MediaFlowContext,
};
use crate::routes::{api_key_allowed, api_key_from_request, cancel_task_browser_sessions};
use crate::video_api_response::{
    completed_video_metadata, openai_video, result_has_video, task_owned_by, video_asset_url,
};
use crate::video_flow_execution::{execute_video_flow, prepare_video_flow_request};
use crate::video_request::OpenAiVideoRequest;
use crate::ServerState;

pub async fn list_videos(
    state: web::Data<ServerState>,
    request: HttpRequest,
    query: web::Query<VideoListQuery>,
) -> impl Responder {
    let Some(owner_hash) = authorized_owner_hash(&state, &request) else {
        return openai_error(401, "authentication_error", "invalid api key");
    };
    let mut tasks = owned_video_tasks(&state, &owner_hash);
    if query.order.as_deref() == Some("asc") {
        tasks.reverse();
    }
    if let Some(after) = query.after.as_deref() {
        if let Some(index) = tasks.iter().position(|task| task.id == after) {
            tasks = tasks.into_iter().skip(index + 1).collect();
        }
    }
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let has_more = tasks.len() > limit;
    tasks.truncate(limit);
    HttpResponse::Ok().json(json!({
        "object": "list",
        "data": tasks.iter().map(openai_video).collect::<Vec<_>>(),
        "has_more": has_more,
    }))
}

pub async fn retrieve_video(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    match owned_video_task(&state, &request, &path.into_inner()) {
        Ok(task) => HttpResponse::Ok().json(openai_video(&task)),
        Err(response) => response,
    }
}

pub async fn cancel_video(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    let video_id = path.into_inner();
    let task = match owned_video_task(&state, &request, &video_id) {
        Ok(task) => task,
        Err(response) => return response,
    };
    if matches!(task.status.as_str(), "completed" | "failed" | "cancelled") {
        return HttpResponse::Conflict().json(json!({
            "error": {
                "message": "任务已经结束，不能取消",
                "type": "invalid_state_error",
                "code": "invalid_state_error"
            }
        }));
    }
    let Some(task) = state.store.cancel_task(&video_id) else {
        return openai_error(404, "not_found", "video not found");
    };
    cancel_task_browser_sessions(&state, &video_id).await;
    HttpResponse::Ok().json(openai_video(&task))
}

pub async fn delete_video(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    let video_id = path.into_inner();
    let task = match owned_video_task(&state, &request, &video_id) {
        Ok(task) => task,
        Err(response) => return response,
    };
    if matches!(task.status.as_str(), "queued" | "processing") {
        return openai_error(409, "invalid_state_error", "运行中的视频请先取消");
    }
    state.store.delete_task(&video_id);
    HttpResponse::Ok().json(json!({
        "id": video_id,
        "object": "video.deleted",
        "deleted": true,
    }))
}

pub async fn retrieve_video_content(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
    query: web::Query<VideoContentQuery>,
) -> impl Responder {
    let task = match owned_video_task(&state, &request, &path.into_inner()) {
        Ok(task) => task,
        Err(response) => return response,
    };
    if task.status != "completed" {
        return openai_error(409, "video_not_ready", "视频尚未生成完成");
    }
    let variant = query.variant.as_deref().unwrap_or("video");
    let Some(url) = video_asset_url(&task, variant) else {
        return openai_error(400, "unsupported_variant", "该视频不支持请求的资源类型");
    };
    let account = task
        .account_id
        .as_deref()
        .and_then(|id| state.store.account_by_id(id))
        .map(|account| state.store.account_with_required_runtime_proxy(account))
        .transpose();
    let proxy_url = match account {
        Ok(Some(account)) => account.proxy_url,
        _ => {
            return openai_error(
                409,
                "proxy_unavailable",
                "原账号或绑定代理已不可用，拒绝自动直连下载",
            )
        }
    };
    let client = match crate::net_client::http_client(&proxy_url) {
        Ok(client) => client,
        Err(message) => return openai_error(502, "download_failed", &message),
    };
    let response = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            return openai_error(
                502,
                "download_failed",
                &format!("平台资源下载失败: HTTP {}", response.status()),
            )
        }
        Err(error) => return openai_error(502, "download_failed", &error.to_string()),
    };
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(if variant == "thumbnail" {
            "image/jpeg"
        } else {
            "video/mp4"
        })
        .to_string();
    let content_length = response.content_length();
    let stream = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(actix_web::error::ErrorBadGateway));
    let mut builder = HttpResponse::Ok();
    builder.insert_header(("content-type", content_type));
    if let Some(length) = content_length {
        builder.insert_header(("content-length", length));
    }
    builder.streaming(stream)
}

pub(crate) async fn run_video_task(
    state: &web::Data<ServerState>,
    body: OpenAiVideoRequest,
    caller_key: String,
    task_id: String,
    first_account_id: String,
) -> Result<Value, ApiError> {
    ensure_task_not_cancelled(state, &task_id)?;
    state.store.start_task(&task_id);
    state
        .store
        .add_task_event(&task_id, "info", "视频任务开始执行".to_string());
    let context = initial_video_context(state, &body, task_id, &first_account_id)?;
    run_video_attempts(state, &body, &caller_key, context).await
}

fn initial_video_context<'a>(
    state: &'a web::Data<ServerState>,
    body: &OpenAiVideoRequest,
    task_id: String,
    first_account_id: &str,
) -> Result<MediaFlowContext<'a>, ApiError> {
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
        .account_by_id(first_account_id)
        .ok_or_else(|| (409, "account_unavailable", "账号已不存在".to_string()))?;
    Ok(MediaFlowContext {
        manifest,
        model,
        account,
        task_id,
    })
}

async fn run_video_attempts(
    state: &web::Data<ServerState>,
    body: &OpenAiVideoRequest,
    caller_key: &str,
    mut context: MediaFlowContext<'_>,
) -> Result<Value, ApiError> {
    let mut excluded = Vec::<String>::new();
    let mut same_account_failures = 0u32;
    loop {
        ensure_task_not_cancelled(state, &context.task_id)?;
        state
            .store
            .set_task_account(&context.task_id, &context.account.id);
        match run_single_video_attempt(state, body, &context).await {
            Ok(result) => return complete_video_attempt(state, body, &context, result),
            Err(message) => {
                if let Some(response) = handle_attempt_failure(
                    state,
                    &mut context,
                    &message,
                    "视频",
                    &mut same_account_failures,
                    Some(caller_key),
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

async fn run_single_video_attempt(
    state: &web::Data<ServerState>,
    body: &OpenAiVideoRequest,
    context: &MediaFlowContext<'_>,
) -> Result<Value, String> {
    let (request, temp_files) = prepare_video_flow_request(
        body,
        context.model,
        context.task_id.clone(),
        &context.account.proxy_url,
    )
    .await
    .map_err(|(_, _, message)| message)?;
    let result = execute_video_flow(state, context.manifest, &context.account, request).await;
    cleanup_temp_files(&temp_files).await;
    result
}

fn complete_video_attempt(
    state: &web::Data<ServerState>,
    body: &OpenAiVideoRequest,
    context: &MediaFlowContext<'_>,
    result: Value,
) -> Result<Value, ApiError> {
    ensure_task_not_cancelled(state, &context.task_id)?;
    if !result_has_video(&result) {
        let message = "插件没有返回可下载视频".to_string();
        state.store.fail_task(&context.task_id, message.clone());
        return Err((502, "video_result_error", message));
    }
    let task = state
        .store
        .task_by_id(&context.task_id)
        .ok_or_else(|| (404, "not_found", "video not found".to_string()))?;
    let metadata = completed_video_metadata(&task, body, &result);
    state.store.mark_account_usage(&context.account.id);
    state.store.complete_task(&context.task_id, metadata);
    state.store.add_audit_log(
        "engine",
        "info",
        "视频任务完成".to_string(),
        format!(
            "task={}, platform={}",
            context.task_id, context.manifest.platform_id
        ),
        Some(context.task_id.clone()),
    );
    Ok(result)
}

fn authorized_owner_hash(state: &ServerState, request: &HttpRequest) -> Option<String> {
    if !api_key_allowed(state, request) {
        return None;
    }
    api_key_from_request(request).map(|key| hash_secret(&key))
}

fn owned_video_task(
    state: &ServerState,
    request: &HttpRequest,
    video_id: &str,
) -> Result<crate::store::TaskRecord, HttpResponse> {
    let owner_hash = authorized_owner_hash(state, request)
        .ok_or_else(|| openai_error(401, "authentication_error", "invalid api key"))?;
    state
        .store
        .task_by_id(video_id)
        .filter(|task| task.task_kind == "video" && task_owned_by(task, &owner_hash))
        .ok_or_else(|| openai_error(404, "not_found", "video not found"))
}

fn owned_video_tasks(state: &ServerState, owner_hash: &str) -> Vec<crate::store::TaskRecord> {
    state
        .store
        .list_tasks(None)
        .into_iter()
        .filter(|task| task.task_kind == "video" && task_owned_by(task, owner_hash))
        .collect()
}

#[derive(Deserialize)]
pub struct VideoListQuery {
    after: Option<String>,
    limit: Option<usize>,
    order: Option<String>,
}

#[derive(Deserialize)]
pub struct VideoContentQuery {
    variant: Option<String>,
}
