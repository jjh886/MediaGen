//! 文本协议适配现有持久生成队列，共用调度、重试、取消和结果查询。
use crate::{request_identity::ProtocolRequest, store::TaskRecord, ServerState};
use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
struct TextRequest {
    endpoint: String,
    body: Value,
    context: ProtocolRequest,
}

pub(crate) async fn enqueue_text(
    state: web::Data<ServerState>,
    key: String,
    body: Value,
    endpoint: &str,
    mut context: ProtocolRequest,
) -> HttpResponse {
    let fail = crate::protocol_api::error;
    if !state.generation_queue_enabled {
        return fail(503, "QUEUE_DISABLED", "异步生成队列未启用");
    }
    let owner = super::owner_key(&key);
    let id = context.task_id.clone();
    // 相同调用方明确重用请求编号时，返回已有任务，不重复入队。
    if state.store.generation_job(&id, Some(&owner)).is_some() {
        return super::api::accepted(&id);
    }
    let model_id = body["model"].as_str().unwrap_or_default();
    let Some((manifest, _, _)) = crate::model_api::resolve(&state, model_id, &key) else {
        return fail(404, "model_not_found", "模型不存在");
    };
    let credential = match state.credential_cipher.encrypt(&key) {
        Ok(value) => value,
        Err(_) => return fail(500, "CREDENTIAL_ERROR", "保存任务凭据失败"),
    };
    let task = TaskRecord {
        id: id.clone(),
        platform: manifest.platform_id,
        task_kind: "text".into(),
        model_id: model_id.into(),
        prompt: String::new(),
        account_id: None,
        status: "pending".into(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        completed_at: None,
    };
    context.queued = true;
    let stored = TextRequest {
        endpoint: endpoint.into(),
        body,
        context,
    };
    let request_json = match serde_json::to_string(&stored) {
        Ok(value) => value,
        Err(_) => return fail(400, "invalid_request", "任务编码失败"),
    };
    let result = state.store.with_account_dispatch(|| {
        if state.store.generation_job(&id, Some(&owner)).is_some() {
            return Ok(());
        }
        if state.store.active_generation_job_count("text") >= super::api::MAX_ACTIVE_GENERATION_JOBS
        {
            return Err((429, "QUEUE_FULL", "异步生成队列已满"));
        }
        let (task, job) = super::records::queued_records(task, request_json, &key, credential);
        state
            .store
            .create_generation_job(task, job)
            .map_err(|_| (500, "STORAGE_ERROR", "保存生成任务失败"))
    });
    match result {
        Ok(()) => {
            state.generation_queue_notify.notify_waiters();
            super::api::accepted(&id)
        }
        Err((status, code, message)) => fail(status, code, message),
    }
}

pub(super) async fn run(
    state: web::Data<ServerState>,
    job: &crate::store::GenerationJobRecord,
    key: String,
) -> Result<Value, (u16, String, String)> {
    let request: TextRequest = serde_json::from_str(&job.request_json)
        .map_err(|_| (400, "invalid_request".into(), "文本任务数据无效".into()))?;
    if !matches!(request.endpoint.as_str(), "chat" | "responses")
        || request.context.task_id != job.task_id
        || !request.context.queued
    {
        return Err((400, "invalid_request".into(), "文本任务上下文无效".into()));
    }
    let response =
        crate::protocol_api::execute(state, request.context, request.body, &request.endpoint, key)
            .await;
    let status = response.status().as_u16();
    let bytes = actix_web::body::to_bytes(response.into_body())
        .await
        .map_err(|_| (500, "invalid_response".into(), "读取文本结果失败".into()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| (500, "invalid_response".into(), "文本结果格式无效".into()))?;
    if status < 400 {
        Ok(value)
    } else {
        Err((
            status,
            value["error"]["code"]
                .as_str()
                .unwrap_or("upstream_error")
                .into(),
            value["error"]["message"]
                .as_str()
                .unwrap_or("文本生成失败")
                .into(),
        ))
    }
}

/// 仅为共享 worker 提供当前可启动数量，实际账号准入沿用协议层并发与能力校验。
pub(super) fn has_capacity(state: &ServerState) -> bool {
    let capacity = state
        .store
        .list_accounts(None)
        .iter()
        .filter(|account| {
            crate::store_helpers::account_ready_for_kind(account, &[], "text")
                && state
                    .store
                    .account_with_required_runtime_proxy((*account).clone())
                    .is_ok()
                && state
                    .plugin_loader
                    .get_plugin(&account.platform)
                    .is_some_and(|plugin| plugin.flows.contains_key("generate_text"))
        })
        .map(|account| u64::from(crate::store_helpers::text_concurrency_limit(account)))
        .sum::<u64>();
    let mut active = state
        .store
        .tasks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|task| task.task_kind == "text" && task.status == "processing")
        .map(|task| task.id.clone())
        .collect::<std::collections::HashSet<_>>();
    active.extend(
        state
            .store
            .generation_jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|job| {
                job.task_kind == "text" && job.state == crate::store::GenerationJobState::Leased
            })
            .map(|job| job.task_id.clone()),
    );
    (active.len() as u64) < capacity
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
