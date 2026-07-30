use std::time::Duration;

use actix_web::web;
use chrono::{Duration as ChronoDuration, Utc};

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
    let caller_key = match state.credential_cipher.decrypt(&job.credential_ciphertext) {
        Ok(value) => value,
        Err(error) => {
            requeue(&state, &job.task_id, "credential", &error, 300);
            return;
        }
    };
    let body = match serde_json::from_str::<OpenAiImageRequest>(&job.request_json) {
        Ok(value) => value,
        Err(error) => {
            finish_invalid_job(&state, &job.task_id, &format!("任务数据无效: {error}"));
            return;
        }
    };
    let result =
        crate::image_api::run_queued_image_flow(state.clone(), body, caller_key, &job.task_id)
            .await;
    finish_execution(&state, &job.task_id, result);
    state.generation_queue_notify.notify_waiters();
}

fn finish_execution(
    state: &web::Data<ServerState>,
    task_id: &str,
    result: Result<(), (u16, &'static str, String)>,
) {
    let Some(task) = state.store.task_by_id(task_id) else {
        requeue(state, task_id, "storage", "任务记录不存在", 120);
        return;
    };
    if task.status == "cancelled" {
        state
            .store
            .finish_generation_job(task_id, GenerationJobState::Cancelled, Some("用户取消"));
        return;
    }
    if result.is_ok() && task.status == "completed" {
        state
            .store
            .finish_generation_job(task_id, GenerationJobState::Completed, None);
        return;
    }
    let (code, message) = match result {
        Err((_, code, message)) => (code, message),
        Ok(()) => ("incomplete", "任务未产生完整结果".to_string()),
    };
    if terminal_failure(&message) {
        state
            .store
            .fail_task(task_id, content_policy_summary(&message));
        state.store.finish_generation_job(
            task_id,
            GenerationJobState::Failed,
            Some(&content_policy_summary(&message)),
        );
        return;
    }
    requeue(
        state,
        task_id,
        code,
        &message,
        retry_seconds(code, &message),
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
