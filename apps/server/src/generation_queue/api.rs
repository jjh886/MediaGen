use std::collections::{HashMap, HashSet};

use actix_web::{http::StatusCode, web, HttpRequest, HttpResponse};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::image_request::OpenAiImageRequest;
use crate::pagination::DEFAULT_PAGE_SIZE;
use crate::routes::{api_key_allowed, api_key_from_request};
use crate::store::{GenerationJobRecord, GenerationJobState};
use crate::ServerState;

use super::{owner_key, queued_image_records};

const MAX_ACTIVE_GENERATION_JOBS: usize = 10_000;
const MAX_BATCH_TASKS: usize = 1_000;

pub(crate) async fn enqueue_image(
    state: web::Data<ServerState>,
    caller_key: String,
    body: OpenAiImageRequest,
) -> HttpResponse {
    let encrypted_key = match state.credential_cipher.encrypt(&caller_key) {
        Ok(value) => value,
        Err(error) => return public_error(500, "CREDENTIAL_ERROR", &error),
    };
    let task_id = match enqueue_image_job(&state, &caller_key, encrypted_key, body) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    HttpResponse::Accepted().json(json!({
        "success": true,
        "data": {
            "taskId": task_id,
            "status": "queued",
            "message": "开始排队生成中",
        }
    }))
}

pub(crate) fn enqueue_image_job(
    state: &ServerState,
    caller_key: &str,
    credential_ciphertext: String,
    body: OpenAiImageRequest,
) -> Result<String, QueueInsertError> {
    if !state.generation_queue_enabled {
        return Err(queue_error(503, "QUEUE_DISABLED", "异步生成队列未启用"));
    }
    let Some((manifest, model)) = state.plugin_loader.resolve_model(&body.model) else {
        return Err(queue_error(404, "MODEL_NOT_FOUND", "生成模型不存在"));
    };
    if model.task_kind != mediagen_core::TaskKind::Image
        || !manifest.flows.contains_key("generate_image")
    {
        return Err(queue_error(400, "FLOW_NOT_FOUND", "模型不支持图片生成"));
    }
    if state.store.active_generation_job_count() >= MAX_ACTIVE_GENERATION_JOBS {
        return Err(queue_error(429, "QUEUE_FULL", "异步生成队列已满"));
    }
    let task_id = format!("task-{}", Uuid::new_v4());
    let now = Utc::now().to_rfc3339();
    let (task, job) = match queued_image_records(
        task_id.clone(),
        manifest.platform_id.clone(),
        &body,
        caller_key,
        credential_ciphertext,
        now,
    ) {
        Ok(records) => records,
        Err(error) => return Err(queue_error(400, "INVALID_REQUEST", &error.to_string())),
    };
    if let Err(error) = state.store.create_generation_job(task, job) {
        return Err(queue_error(500, "STORAGE_ERROR", &error));
    }
    state.generation_queue_notify.notify_waiters();
    Ok(task_id)
}

pub(crate) struct QueueInsertError {
    pub(crate) status: u16,
    pub(crate) code: &'static str,
    pub(crate) detail: String,
}

impl QueueInsertError {
    fn into_response(self) -> HttpResponse {
        public_error(self.status, self.code, &self.detail)
    }
}

fn queue_error(status: u16, code: &'static str, detail: &str) -> QueueInsertError {
    QueueInsertError {
        status,
        code,
        detail: detail.to_string(),
    }
}

pub(crate) async fn admin_list_queue(
    state: web::Data<ServerState>,
    query: web::Query<ListQuery>,
) -> HttpResponse {
    let (jobs, total) = state.store.list_all_generation_jobs(
        query.limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, 100),
        query.offset.unwrap_or(0),
    );
    HttpResponse::Ok().json(json!({
        "data": queue_response(&state, jobs, total, query.offset.unwrap_or(0))
    }))
}

pub(crate) async fn list_queue(
    state: web::Data<ServerState>,
    request: HttpRequest,
    query: web::Query<ListQuery>,
) -> HttpResponse {
    let Some(owner) = authorized_owner(&state, &request) else {
        return public_error(401, "UNAUTHORIZED", "无效的 Bearer Token");
    };
    let offset = query.offset.unwrap_or(0);
    let (jobs, total) = state.store.list_generation_jobs(
        &owner,
        query.limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, 100),
        offset,
    );
    HttpResponse::Ok().json(json!({
        "success": true,
        "data": queue_response(&state, jobs, total, offset),
    }))
}

pub(crate) async fn admin_queue_action(
    state: web::Data<ServerState>,
    body: web::Json<QueueActionRequest>,
) -> HttpResponse {
    apply_action(&state, None, body.into_inner(), false)
}

pub(crate) async fn queue_action(
    state: web::Data<ServerState>,
    request: HttpRequest,
    body: web::Json<QueueActionRequest>,
) -> HttpResponse {
    let Some(owner) = authorized_owner(&state, &request) else {
        return public_error(401, "UNAUTHORIZED", "无效的 Bearer Token");
    };
    apply_action(&state, Some(&owner), body.into_inner(), true)
}

pub(crate) async fn task_detail(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    let Some(owner) = authorized_owner(&state, &request) else {
        return public_error(401, "UNAUTHORIZED", "无效的 Bearer Token");
    };
    let task_id = path.into_inner();
    match build_task_detail(&state, &owner, &task_id, &queue_positions(&state, &owner)).await {
        Some(detail) => HttpResponse::Ok().json(json!({ "success": true, "data": detail })),
        None => public_error(404, "TASK_NOT_FOUND", "异步任务不存在"),
    }
}

pub(crate) async fn batch_task_details(
    state: web::Data<ServerState>,
    request: HttpRequest,
    body: web::Json<BatchTaskDetailsRequest>,
) -> HttpResponse {
    let Some(owner) = authorized_owner(&state, &request) else {
        return public_error(401, "UNAUTHORIZED", "无效的 Bearer Token");
    };
    let task_ids = normalize_task_ids(&body.task_ids);
    if !(1..=MAX_BATCH_TASKS).contains(&task_ids.len()) {
        return public_error(400, "INVALID_REQUEST", "任务数量必须在 1 到 1000 之间");
    }
    let positions = queue_positions(&state, &owner);
    let mut items = Vec::new();
    let mut missing = Vec::new();
    for task_id in task_ids {
        match build_task_detail(&state, &owner, &task_id, &positions).await {
            Some(item) => items.push(item),
            None => missing.push(task_id),
        }
    }
    HttpResponse::Ok().json(json!({
        "success": true,
        "data": { "items": items, "notFoundTaskIds": missing }
    }))
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueActionRequest {
    action: String,
    task_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchTaskDetailsRequest {
    task_ids: Vec<String>,
}

fn apply_action(
    state: &ServerState,
    owner: Option<&str>,
    body: QueueActionRequest,
    public_response: bool,
) -> HttpResponse {
    let task_ids = normalize_task_ids(&body.task_ids);
    let cancel_callbacks = if body.action == "cancel" {
        super::callback::pending_cancel_callbacks(state, owner, &task_ids)
    } else {
        Vec::new()
    };
    let affected = match state
        .store
        .apply_generation_queue_action(owner, &body.action, &task_ids)
    {
        Ok(value) => value,
        Err(error) if public_response => return public_error(409, "QUEUE_ACTION_FAILED", &error),
        Err(error) => return HttpResponse::Conflict().json(json!({ "error": error })),
    };
    state.generation_queue_notify.notify_waiters();
    if affected > 0 && !cancel_callbacks.is_empty() {
        super::callback::spawn_cancelled_batch((*state).clone(), cancel_callbacks);
    }
    if public_response {
        HttpResponse::Ok().json(json!({
            "success": true,
            "data": { "action": body.action, "affected": affected }
        }))
    } else {
        HttpResponse::Ok().json(json!({ "data": { "affected": affected } }))
    }
}

fn queue_response(
    state: &ServerState,
    jobs: Vec<GenerationJobRecord>,
    total: usize,
    offset: usize,
) -> serde_json::Value {
    let items = jobs
        .iter()
        .enumerate()
        .filter_map(|(index, job)| queue_item(state, job, offset + index + 1))
        .collect::<Vec<_>>();
    json!({ "total": total, "items": items })
}

fn queue_item(
    state: &ServerState,
    job: &GenerationJobRecord,
    position: usize,
) -> Option<serde_json::Value> {
    let request = serde_json::from_str::<OpenAiImageRequest>(&job.request_json).ok()?;
    let task = state.store.task_by_id(&job.task_id)?;
    Some(json!({
        "taskId": job.task_id,
        "platform": task.platform,
        "modelId": request.model,
        "prompt": request.prompt_text(),
        "status": "queued",
        "position": position,
        "queuedAt": job.created_at,
    }))
}

async fn build_task_detail(
    state: &ServerState,
    owner: &str,
    task_id: &str,
    positions: &HashMap<String, usize>,
) -> Option<serde_json::Value> {
    let job = state.store.generation_job(task_id, Some(owner))?;
    let task = state.store.task_by_id(task_id)?;
    let status = visible_status(&job.state, &task.status);
    let metadata = task
        .result_data
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
    let result = if status == "completed" {
        match super::result::read(state, task_id).await {
            Ok(Some(value)) => Some(value),
            Ok(None) => metadata,
            Err(_) => Some(
                json!({"error":{"code":"RESULT_STORAGE_ERROR","message":"图片结果暂时无法读取"}}),
            ),
        }
    } else {
        metadata
    };
    Some(json!({
        "taskId": task.id,
        "platform": task.platform,
        "modelId": task.model_id,
        "prompt": task.prompt,
        "status": status,
        "position": positions.get(task_id),
        "result": result,
        "failureReason": terminal_reason(&task.status, task.error_message.as_deref()),
        "queuedAt": job.created_at,
        "startedAt": job.started_at,
        "updatedAt": job.updated_at,
        "completedAt": job.completed_at.or(task.completed_at),
    }))
}

fn queue_positions(state: &ServerState, owner: &str) -> HashMap<String, usize> {
    state
        .store
        .list_generation_jobs(owner, MAX_ACTIVE_GENERATION_JOBS, 0)
        .0
        .into_iter()
        .enumerate()
        .map(|(index, job)| (job.task_id, index + 1))
        .collect()
}

fn visible_status(job: &GenerationJobState, task_status: &str) -> &'static str {
    match job {
        GenerationJobState::Queued | GenerationJobState::RetryWait => "queued",
        GenerationJobState::Leased if task_status == "completed" => "completed",
        GenerationJobState::Leased if task_status == "failed" => "failed",
        GenerationJobState::Leased if task_status == "cancelled" => "cancelled",
        GenerationJobState::Leased => "processing",
        GenerationJobState::Completed => "completed",
        GenerationJobState::Failed => "failed",
        GenerationJobState::Cancelled => "cancelled",
    }
}

fn terminal_reason(status: &str, error: Option<&str>) -> Option<String> {
    matches!(status, "failed" | "cancelled").then(|| error.unwrap_or("任务已结束").to_string())
}

fn authorized_owner(state: &ServerState, request: &HttpRequest) -> Option<String> {
    if !api_key_allowed(state, request) {
        return None;
    }
    api_key_from_request(request).map(|token| owner_key(&token))
}

fn normalize_task_ids(task_ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    task_ids
        .iter()
        .map(|task_id| task_id.trim())
        .filter(|task_id| !task_id.is_empty() && seen.insert((*task_id).to_string()))
        .map(str::to_string)
        .collect()
}

fn public_error(status: u16, code: &str, detail: &str) -> HttpResponse {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    HttpResponse::build(status).json(json!({
        "success": false,
        "error": { "code": code, "detail": detail }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_ids_are_trimmed_deduplicated_and_ordered() {
        let ids = vec![
            " task-2 ".to_string(),
            "".to_string(),
            "task-1".to_string(),
            "task-2".to_string(),
        ];
        assert_eq!(normalize_task_ids(&ids), vec!["task-2", "task-1"]);
    }

    #[test]
    fn queue_limit_matches_aiduanju() {
        assert_eq!(MAX_ACTIVE_GENERATION_JOBS, 10_000);
    }

    #[tokio::test]
    async fn completed_task_exposes_image_before_worker_finishes_lease() {
        let root = std::env::temp_dir().join(format!("queue-result-race-{}", Uuid::new_v4()));
        let state = crate::test_fixtures::server_state(
            root.clone(),
            mediagen_plugin_runtime::PluginLoader::empty(),
        );
        let body: OpenAiImageRequest =
            serde_json::from_value(json!({"model":"test", "prompt":"image"})).unwrap();
        let (task, mut job) = queued_image_records(
            "task-race".into(),
            "test".into(),
            &body,
            "key",
            String::new(),
            Utc::now().to_rfc3339(),
        )
        .unwrap();
        job.state = GenerationJobState::Leased;
        state.store.create_generation_job(task, job).unwrap();
        let result = json!({"created":1,"data":[{"b64_json":"image-base64"}]});
        super::super::result::save(&state, "task-race", &result)
            .await
            .unwrap();
        state
            .store
            .complete_task("task-race", json!({"image_count":1}));
        let detail = build_task_detail(&state, &owner_key("key"), "task-race", &HashMap::new())
            .await
            .unwrap();
        assert_eq!(detail["status"], "completed");
        assert_eq!(detail["result"], result);
        assert_eq!(state.store.recover_leased_generation_jobs(Utc::now()), 1);
        assert_eq!(
            state.store.generation_job("task-race", None).unwrap().state,
            GenerationJobState::Completed
        );
        assert_eq!(
            state.store.task_by_id("task-race").unwrap().status,
            "completed"
        );
        assert!(build_task_detail(
            &state,
            &owner_key("another-key"),
            "task-race",
            &HashMap::new()
        )
        .await
        .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
