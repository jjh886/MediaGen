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

pub(super) fn dispatch_available(state: &web::Data<ServerState>) {
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
                    if job.task_kind == "text" {
                        text_available
                    } else {
                        permit.is_some()
                    }
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
    }
}

async fn execute_job(
    state: web::Data<ServerState>,
    job: crate::store::GenerationJobRecord,
    _permit: Option<crate::image_task_queue::ImageTaskPermit>,
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

    #[actix_web::test]
    async fn permission_and_zero_quota_stop_queue_without_becoming_safety_errors() {
        let root = std::env::temp_dir().join(format!("queue-permission-{}", uuid::Uuid::new_v4()));
        let state = web::Data::new(crate::test_fixtures::server_state(
            root,
            mediagen_plugin_runtime::PluginLoader::empty(),
        ));
        let body: OpenAiImageRequest =
            serde_json::from_value(serde_json::json!({"model":"test-image","prompt":"circle"}))
                .unwrap();
        for (status, code) in [
            (403, "account_permission_denied"),
            (429, "insufficient_quota"),
        ] {
            let id = uuid::Uuid::new_v4().to_string();
            let (task, job) = super::super::queued_image_records(
                id.clone(),
                "test".into(),
                &body,
                "key",
                "encrypted".into(),
                Utc::now().to_rfc3339(),
            )
            .unwrap();
            state.store.create_generation_job(task, job).unwrap();
            finish_execution(
                &state,
                &id,
                Some(&body),
                Err((status, code.to_string(), "账号未获授权或额度为零".into())),
            )
            .await;
            let job = state.store.generation_job(&id, None).unwrap();
            assert_eq!(job.state, GenerationJobState::Failed);
            assert_eq!(job.retry_count, 0);
            assert_eq!(
                job.last_error_summary.as_deref(),
                Some("账号未获授权或额度为零")
            );
        }
    }

    #[actix_web::test]
    async fn image_and_text_queues_run_independently_when_other_kind_is_full() {
        use serde_json::json;
        let original = crate::protocol_api_tests::state();
        let images = crate::image_api_tests::queue_state("independent-queues");
        let mut config = original.get_ref().clone();
        let plugins = config
            .plugin_loader
            .get_plugin_dir("test")
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        std::fs::create_dir_all(plugins.join("fake")).unwrap();
        std::fs::copy(
            images
                .plugin_loader
                .get_plugin_dir("fake")
                .unwrap()
                .join("manifest.json"),
            plugins.join("fake/manifest.json"),
        )
        .unwrap();
        config.plugin_loader = std::sync::Arc::new(
            mediagen_plugin_runtime::PluginLoader::load_from(&plugins).unwrap(),
        );
        config
            .store
            .insert_account(images.store.account_by_id("acc-queue").unwrap());
        config.store.update_settings(crate::store::SettingsUpdate {
            image_task_concurrency: Some(1),
            ..Default::default()
        });
        let state = web::Data::new(config);
        let permit = state
            .image_task_queue
            .try_acquire_background(&state.store.settings())
            .unwrap();
        let image_id = super::super::enqueue_image_job(
            &state,
            "key-a",
            state.credential_cipher.encrypt("key-a").unwrap(),
            serde_json::from_value(json!({"model":"fake-image","prompt":"draw"})).unwrap(),
        )
        .unwrap_or_else(|_| panic!("图片任务入队失败"));
        let body = json!({"model":"text-model","input":"test","async":true});
        let request = actix_web::test::TestRequest::post()
            .uri("/v1/responses")
            .to_http_request();
        let first = crate::request_identity::ProtocolRequest::from_http(&request, "key-a", &body);
        let text_id = first.task_id.clone();
        assert_eq!(
            super::super::enqueue_text(
                state.clone(),
                "key-a".into(),
                body.clone(),
                "responses",
                first
            )
            .await
            .status(),
            202
        );
        dispatch_available(&state);
        wait_completed(&state, &text_id).await;
        assert_eq!(
            state.store.generation_job(&image_id, None).unwrap().state,
            GenerationJobState::Queued
        );
        drop(permit);
        let manifest = state.plugin_loader.get_plugin("test").unwrap();
        let busy = (0..3)
            .map(|i| {
                crate::protocol_task::reserve(
                    &state,
                    manifest,
                    "text-model",
                    "text",
                    "key-a",
                    &format!("busy-{i}"),
                    None,
                )
                .unwrap()
                .1
            })
            .collect::<Vec<_>>();
        let second = crate::request_identity::ProtocolRequest::from_http(&request, "key-a", &body);
        let second_id = second.task_id.clone();
        assert_eq!(
            super::super::enqueue_text(state.clone(), "key-a".into(), body, "responses", second)
                .await
                .status(),
            202
        );
        dispatch_available(&state);
        wait_completed(&state, &image_id).await;
        assert_eq!(
            state.store.generation_job(&second_id, None).unwrap().state,
            GenerationJobState::Queued
        );
        drop(busy);
        dispatch_available(&state);
        wait_completed(&state, &second_id).await;
    }

    async fn wait_completed(state: &ServerState, id: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.store.generation_job(id, None).unwrap().state
                != GenerationJobState::Completed
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

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
