use std::time::Duration;

use actix_web::web;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::Value;

use crate::image_request::OpenAiImageRequest;
use crate::store::GenerationJobState;
use crate::ServerState;

const DISPATCH_INTERVAL: Duration = Duration::from_secs(2);
const LEASE_HOURS: i64 = 6;

pub(crate) fn start_worker(state: web::Data<ServerState>) {
    let recovered = state.store.recover_leased_generation_jobs(Utc::now());
    if recovered > 0 {
        eprintln!("[mediagen-server] 已恢复 {recovered} 个异常退出前正在执行的生成任务");
    }
    actix_web::rt::spawn(async move {
        loop {
            dispatch_available(&state);
            tokio::select! {
                _ = state.generation_queue_notify.notified() => {}
                _ = tokio::time::sleep(DISPATCH_INTERVAL) => {}
            }
        }
    });
}

fn dispatch_available(state: &web::Data<ServerState>) {
    loop {
        if state.image_task_queue.direct_waiter_count() > 0 {
            return;
        }
        let settings = state.store.settings();
        let Some(permit) = state.image_task_queue.try_acquire_background(&settings) else {
            return;
        };
        let now = Utc::now();
        let job = state
            .store
            .claim_next_generation_job(now, now + ChronoDuration::hours(LEASE_HOURS));
        let Some(job) = job else {
            return;
        };
        let state = state.clone();
        actix_web::rt::spawn(async move {
            execute_job(state, job, permit).await;
        });
    }
}

async fn execute_job(
    state: web::Data<ServerState>,
    job: crate::store::GenerationJobRecord,
    _permit: crate::image_task_queue::ImageTaskPermit,
) {
    let caller_key = match job.credential_ciphertext.is_empty() {
        true => String::new(),
        false => match state.credential_cipher.decrypt(&job.credential_ciphertext) {
            Ok(value) => value,
            Err(error) => {
                requeue(&state, &job.task_id, "credential", &error, 300);
                return;
            }
        },
    };
    let body = match serde_json::from_str::<OpenAiImageRequest>(&job.request_json) {
        Ok(value) => value,
        Err(error) => {
            finish_invalid_job(&state, &job.task_id, &format!("任务数据无效: {error}"));
            return;
        }
    };
    let callback_body = body.clone();
    let result =
        crate::image_api::run_queued_image_flow(state.clone(), body, caller_key, &job.task_id)
            .await;
    finish_execution(&state, &job.task_id, &callback_body, result).await;
    state.generation_queue_notify.notify_waiters();
}

async fn finish_execution(
    state: &web::Data<ServerState>,
    task_id: &str,
    body: &OpenAiImageRequest,
    result: Result<Value, (u16, &'static str, String)>,
) {
    let Some(task) = state.store.task_by_id(task_id) else {
        requeue(state, task_id, "storage", "任务记录不存在", 120);
        return;
    };
    if task.status == "cancelled" {
        finish_cancelled(state, task_id, body);
        return;
    }
    match result {
        Err((_, "RESULT_STORAGE_ERROR", message)) => {
            finish_terminal_failure(state, task_id, body, "RESULT_STORAGE_ERROR", &message)
        }
        Ok(response) if task.status == "completed" => {
            finish_completed(state, task_id, body, response).await
        }
        Ok(_) => requeue(state, task_id, "incomplete", "任务未产生完整结果", 120),
        Err((_, code, message)) if terminal_failure(&message) => {
            finish_terminal_failure(state, task_id, body, code, &message)
        }
        Err((_, code, message)) => requeue(
            state,
            task_id,
            code,
            &message,
            retry_seconds(code, &message),
        ),
    }
}

fn finish_cancelled(state: &web::Data<ServerState>, task_id: &str, body: &OpenAiImageRequest) {
    state
        .store
        .finish_generation_job(task_id, GenerationJobState::Cancelled, Some("用户取消"));
    super::callback::spawn_cancelled(state.get_ref().clone(), task_id.to_string(), body.clone());
    crate::weixin::delivery::queue_terminal_status(
        state.get_ref(),
        task_id,
        "cancelled",
        Some("用户取消"),
    );
}

async fn finish_completed(
    state: &web::Data<ServerState>,
    task_id: &str,
    body: &OpenAiImageRequest,
    response: Value,
) {
    state
        .store
        .finish_generation_job(task_id, GenerationJobState::Completed, None);
    crate::weixin::delivery::queue_completed_response(state.get_ref(), task_id, &response).await;
    super::callback::spawn_completed(
        state.get_ref().clone(),
        task_id.to_string(),
        body.clone(),
        response,
    );
}

fn finish_terminal_failure(
    state: &web::Data<ServerState>,
    task_id: &str,
    body: &OpenAiImageRequest,
    code: &'static str,
    message: &str,
) {
    let summary = content_policy_summary(message);
    state.store.fail_task(task_id, summary.clone());
    state
        .store
        .finish_generation_job(task_id, GenerationJobState::Failed, Some(&summary));
    crate::weixin::delivery::queue_terminal_status(
        state.get_ref(),
        task_id,
        "failed",
        Some(&summary),
    );
    super::callback::spawn_failed(
        state.get_ref().clone(),
        task_id.to_string(),
        body.clone(),
        code,
        &summary,
    );
}

fn requeue(
    state: &web::Data<ServerState>,
    task_id: &str,
    category: &str,
    error: &str,
    delay_seconds: i64,
) {
    state.store.requeue_generation_job(
        task_id,
        Utc::now() + ChronoDuration::seconds(delay_seconds.max(1)),
        category,
        &sanitize_error(error),
    );
}

fn finish_invalid_job(state: &web::Data<ServerState>, task_id: &str, error: &str) {
    state.store.fail_task(task_id, error.to_string());
    state
        .store
        .finish_generation_job(task_id, GenerationJobState::Failed, Some(error));
    crate::weixin::delivery::queue_terminal_status(state.get_ref(), task_id, "failed", Some(error));
}

fn retry_seconds(code: &str, error: &str) -> i64 {
    match code {
        "account_unavailable" => 60,
        "verification_required" => 300,
        "proxy_unavailable" => 120,
        _ if rate_limited(error) => 60,
        _ if transient_runtime_error(error) => 30,
        _ => 120,
    }
}

fn transient_runtime_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    ["network", "timeout", "connection", "浏览器", "窗口"]
        .iter()
        .any(|value| lower.contains(value))
}

fn rate_limited(error: &str) -> bool {
    let lower = error.to_lowercase();
    ["rate limit", "429", "频繁", "限额"]
        .iter()
        .any(|value| lower.contains(value))
}

fn terminal_failure(error: &str) -> bool {
    let lower = error.to_lowercase();
    [
        "content policy",
        "safety policy",
        "sexual",
        "minor safety",
        "内容安全",
        "色情",
        "未成年人",
    ]
    .iter()
    .any(|value| lower.contains(value))
}

fn sanitize_error(error: &str) -> String {
    error.chars().take(500).collect()
}

fn content_policy_summary(error: &str) -> String {
    let lower = error.to_lowercase();
    let reason = if lower.contains("sexual") || error.contains("色情") {
        "色情内容"
    } else if lower.contains("minor") || error.contains("未成年人") {
        "未成年人安全"
    } else {
        "内容安全"
    };
    format!("平台安全规则拒绝：{reason}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_timing_matches_failure_category() {
        assert_eq!(retry_seconds("account_unavailable", "none"), 60);
        assert_eq!(retry_seconds("plugin_execution_failed", "HTTP 429"), 60);
        assert_eq!(
            retry_seconds("plugin_execution_failed", "network timeout"),
            30
        );
        assert_eq!(retry_seconds("plugin_execution_failed", "unknown"), 120);
    }
}
