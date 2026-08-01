use std::time::Duration;

use chrono::Utc;
use reqwest::header::USER_AGENT;
use serde_json::{json, Value};

use crate::image_request::OpenAiImageRequest;
use crate::store::GenerationJobState;
use crate::ServerState;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(15);
const CALLBACK_ATTEMPTS: usize = 3;
const CALLBACK_USER_AGENT: &str = "MediaGen-Generation-Callback/1.0";

pub(crate) struct CallbackInput {
    pub(crate) task_id: String,
    pub(crate) body: OpenAiImageRequest,
}

pub(crate) fn pending_cancel_callbacks(
    state: &ServerState,
    owner: Option<&str>,
    task_ids: &[String],
) -> Vec<CallbackInput> {
    task_ids
        .iter()
        .filter_map(|task_id| callback_input_for_pending_job(state, owner, task_id))
        .collect()
}

pub(crate) fn spawn_completed(
    state: ServerState,
    task_id: String,
    body: OpenAiImageRequest,
    data: Value,
) {
    let payload = json!({
        "object": "generation.task.callback",
        "event": "image_generation.completed",
        "task_id": task_id,
        "status": "completed",
        "created": Utc::now().timestamp(),
        "model": body.model.clone(),
        "data": data,
    });
    spawn_delivery(state, task_id, body, payload);
}

pub(crate) fn spawn_failed(
    state: ServerState,
    task_id: String,
    body: OpenAiImageRequest,
    code: &str,
    message: &str,
) {
    let payload = json!({
        "object": "generation.task.callback",
        "event": "image_generation.failed",
        "task_id": task_id,
        "status": "failed",
        "created": Utc::now().timestamp(),
        "model": body.model.clone(),
        "error": { "code": code, "message": message },
    });
    spawn_delivery(state, task_id, body, payload);
}

pub(crate) fn spawn_cancelled(state: ServerState, task_id: String, body: OpenAiImageRequest) {
    let payload = json!({
        "object": "generation.task.callback",
        "event": "image_generation.cancelled",
        "task_id": task_id,
        "status": "cancelled",
        "created": Utc::now().timestamp(),
        "model": body.model.clone(),
        "error": { "code": "task_cancelled", "message": "用户取消" },
    });
    spawn_delivery(state, task_id, body, payload);
}

pub(crate) fn spawn_cancelled_batch(state: ServerState, items: Vec<CallbackInput>) {
    for item in items {
        spawn_cancelled(state.clone(), item.task_id, item.body);
    }
}

fn callback_input_for_pending_job(
    state: &ServerState,
    owner: Option<&str>,
    task_id: &str,
) -> Option<CallbackInput> {
    let job = state.store.generation_job(task_id, owner)?;
    if !matches!(
        job.state,
        GenerationJobState::Queued | GenerationJobState::RetryWait
    ) {
        return None;
    }
    let body = serde_json::from_str::<OpenAiImageRequest>(&job.request_json).ok()?;
    body.callback_url().map(|_| CallbackInput {
        task_id: task_id.to_string(),
        body,
    })
}

fn spawn_delivery(state: ServerState, task_id: String, body: OpenAiImageRequest, payload: Value) {
    let Some(callback_url) = body.callback_url() else {
        return;
    };
    actix_web::rt::spawn(async move {
        let result = post_callback_with_retries(&callback_url, &payload).await;
        record_delivery_result(&state, &task_id, &callback_url, result);
    });
}

async fn post_callback_with_retries(callback_url: &str, payload: &Value) -> Result<(), String> {
    let mut last_error = String::new();
    for attempt in 1..=CALLBACK_ATTEMPTS {
        match post_callback_once(callback_url, payload).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        if attempt < CALLBACK_ATTEMPTS {
            tokio::time::sleep(Duration::from_secs(attempt as u64)).await;
        }
    }
    Err(last_error)
}

async fn post_callback_once(callback_url: &str, payload: &Value) -> Result<(), String> {
    // 回调失败只记录事件，不反向改变生成任务终态，避免图片已生成却被误判为失败。
    let client = reqwest::Client::builder()
        .timeout(CALLBACK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(callback_url)
        .header(USER_AGENT, CALLBACK_USER_AGENT)
        .json(payload)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {status}"))
    }
}

fn record_delivery_result(
    state: &ServerState,
    task_id: &str,
    callback_url: &str,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => state
            .store
            .add_task_event(task_id, "info", "异步回调通知成功".to_string()),
        Err(error) => {
            let summary = format!("异步回调通知失败: {error}");
            state.store.add_task_event(task_id, "warn", summary.clone());
            state.store.add_audit_log(
                "engine",
                "warn",
                "异步回调通知失败".to_string(),
                format!(
                    "task={task_id}, url={}, error={error}",
                    safe_url(callback_url)
                ),
                Some(task_id.to_string()),
            );
        }
    }
}

fn safe_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return "<invalid callback_url>".to_string();
    };
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_url_removes_sensitive_parts() {
        assert_eq!(
            safe_url("https://example.test/cb?token=secret#frag"),
            "https://example.test/cb"
        );
    }
}
