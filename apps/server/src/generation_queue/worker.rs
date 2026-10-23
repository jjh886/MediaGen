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
            let queued = state.generation_queue_notify.notified();
            let released = state.store.dispatch_notify.notified();
            tokio::pin!(queued, released);
            queued.as_mut().enable();
            released.as_mut().enable();
            dispatch_available(&state).await;
            tokio::select! {
                _ = queued => {}
                _ = released => {}
                _ = tokio::time::sleep(DISPATCH_INTERVAL) => {}
            }
        }
    });
}

pub(super) async fn dispatch_available(state: &web::Data<ServerState>) {
    loop {
        let settings = state.store.settings();
        let permit = state.image_task_queue.try_acquire_background(&settings);
        let now = Utc::now();
        let job = state.store.with_account_dispatch(|| {
            let text_available = super::text::has_capacity(state);
            state.store.claim_generation_job_matching(
                now,
                now + ChronoDuration::hours(LEASE_HOURS),
                |job| {
                    (if job.task_kind == "text" {
                        text_available
                    } else {
                        permit.is_some()
                    }) && super::records::runnable(state, job)
                },
            )
        });
        let Some(job) = job else {
            return;
        };
        let permit = if job.task_kind == "text" {
            drop(permit);
            None
        } else {
            permit
        };
        let state = state.clone();
        actix_web::rt::spawn(async move {
            execute_job(state, job, permit).await;
        });
        // 让执行器先登记账号占用，再领取下一条任务。
        tokio::task::yield_now().await;
    }
}

async fn execute_job(
    state: web::Data<ServerState>,
    job: crate::store::GenerationJobRecord,
    permit: Option<crate::image_task_queue::ImageTaskPermit>,
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
    let (callback_body, result) = if job.task_kind == "text" {
        (
            None,
            super::text::run(state.clone(), &job, caller_key).await,
        )
    } else {
        let body = match serde_json::from_str::<OpenAiImageRequest>(&job.request_json) {
            Ok(value) => value,
            Err(_) => {
                finish_invalid_job(&state, &job.task_id, "任务数据无效");
                return;
            }
        };
        let result = crate::image_api::run_queued_image_flow(
            state.clone(),
            body.clone(),
            caller_key,
            &job.task_id,
        )
        .await
        .map_err(|(status, code, message)| (status, code.to_string(), message));
        (Some(body), result)
    };
    finish_execution(&state, &job.task_id, callback_body.as_ref(), result).await;
    drop(permit);
    state.generation_queue_notify.notify_waiters();
}

async fn finish_execution(
    state: &web::Data<ServerState>,
    task_id: &str,
    body: Option<&OpenAiImageRequest>,
    result: Result<Value, (u16, String, String)>,
) {
    let Some(task) = state.store.task_by_id(task_id) else {
        requeue(state, task_id, "storage", "任务记录不存在", 120);
        return;
    };
    if task.status == "cancelled" {
        finish_cancelled(state, task_id, body);
        return;
    }
    if result.as_ref().is_err_and(|(status, _, _)| *status == 429)
        && task
            .account_id
            .as_deref()
            .and_then(|id| state.store.account_by_id(id))
            .is_some_and(|account| {
                !crate::store_helpers::account_ready_for_kind(&account, &[], &task.task_kind)
                    && crate::store_quota::account_can_wait_for_kind(&account, &task.task_kind)
            })
    {
        requeue(
            state,
            task_id,
            "account_limited",
            "账号限额中，等待恢复后继续原任务",
            1,
        );
        return;
    }
    match result {
        Err((status, code, message))
            if status == 403
                || code == "insufficient_quota"
                || (body.is_none() && matches!(status, 400 | 401 | 404 | 422)) =>
        {
            finish_terminal_failure(state, task_id, body, &code, &message)
        }
        Ok(response) if task.status == "completed" => {
            finish_completed(state, task_id, body, response).await
        }
        Ok(_) => requeue(state, task_id, "incomplete", "任务未产生完整结果", 120),
        Err((_, code, message)) if terminal_failure(&message) => {
            finish_terminal_failure(state, task_id, body, &code, &message)
        }
        Err((_, code, message)) => requeue(
            state,
            task_id,
            &code,
            &message,
            retry_seconds(&code, &message),
        ),
    }
}

fn finish_cancelled(
    state: &web::Data<ServerState>,
    task_id: &str,
    body: Option<&OpenAiImageRequest>,
) {
    state
        .store
        .finish_generation_job(task_id, GenerationJobState::Cancelled, Some("用户取消"));
    if let Some(body) = body {
        super::callback::spawn_cancelled(
            state.get_ref().clone(),
            task_id.to_string(),
            body.clone(),
        );
    }
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
    body: Option<&OpenAiImageRequest>,
    response: Value,
) {
    state
        .store
        .finish_generation_job(task_id, GenerationJobState::Completed, None);
    crate::weixin::delivery::queue_completed_response(state.get_ref(), task_id, &response).await;
    if let Some(body) = body {
        super::callback::spawn_completed(
            state.get_ref().clone(),
            task_id.to_string(),
            body.clone(),
            response,
        );
    }
}

fn finish_terminal_failure(
    state: &web::Data<ServerState>,
    task_id: &str,
    body: Option<&OpenAiImageRequest>,
    code: &str,
    message: &str,
) {
    let summary = if terminal_failure(message) {
        content_policy_summary(message)
    } else {
        sanitize_error(message)
    };
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
    if let Some(body) = body {
        super::callback::spawn_failed(
            state.get_ref().clone(),
            task_id.to_string(),
            body.clone(),
            code,
            &summary,
        );
    }
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
        "account_unavailable" => 1,
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
#[path = "worker_tests.rs"]
mod tests;
