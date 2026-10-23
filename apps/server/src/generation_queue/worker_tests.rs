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
    config.plugin_loader =
        std::sync::Arc::new(mediagen_plugin_runtime::PluginLoader::load_from(&plugins).unwrap());
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
    dispatch_available(&state).await;
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
    dispatch_available(&state).await;
    wait_completed(&state, &image_id).await;
    assert_eq!(
        state.store.generation_job(&second_id, None).unwrap().state,
        GenerationJobState::Queued
    );
    drop(busy);
    dispatch_available(&state).await;
    wait_completed(&state, &second_id).await;
}

async fn wait_completed(state: &ServerState, id: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while state.store.generation_job(id, None).unwrap().state != GenerationJobState::Completed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn retry_timing_matches_failure_category() {
    assert_eq!(retry_seconds("account_unavailable", "none"), 1);
    assert_eq!(retry_seconds("plugin_execution_failed", "HTTP 429"), 60);
    assert_eq!(
        retry_seconds("plugin_execution_failed", "network timeout"),
        30
    );
    assert_eq!(retry_seconds("plugin_execution_failed", "unknown"), 120);
}

fn scheduling_state(
    kind: &str,
    limit: u32,
) -> (
    web::Data<ServerState>,
    &'static str,
    &'static str,
    &'static str,
) {
    let (mut config, key, model, account) = if kind == "text" {
        let state = crate::protocol_api_tests::state();
        let path = state
            .plugin_loader
            .get_plugin_dir("test")
            .unwrap()
            .join("scripts/text.js");
        let script = std::fs::read_to_string(&path).unwrap().replacen(
            "function main(ctx) {",
            "function main(ctx) { host.sleep(80);",
            1,
        );
        std::fs::write(path, script).unwrap();
        (state.get_ref().clone(), "key-a", "text-model", "account")
    } else {
        (
            crate::image_api_tests::queue_state("auto-scheduling"),
            "image-key",
            "fake-image",
            "acc-queue",
        )
    };
    if kind == "image" {
        let directory = config.plugin_loader.get_plugin_dir("fake").unwrap();
        let path = directory.join("manifest.json");
        let raw = std::fs::read_to_string(&path).unwrap().replacen(
            "function main(ctx) {",
            "function main(ctx) { host.sleep(80);",
            1,
        );
        std::fs::write(path, raw).unwrap();
        config.plugin_loader = std::sync::Arc::new(
            mediagen_plugin_runtime::PluginLoader::load_from(directory.parent().unwrap()).unwrap(),
        );
    }
    config.store.update_account(
        account,
        crate::store::AccountUpdate {
            concurrency_limit: Some(limit),
            text_concurrency_limit: Some(limit),
            ..Default::default()
        },
    );
    config.store.update_settings(crate::store::SettingsUpdate {
        image_task_concurrency: Some(8),
        ..Default::default()
    });
    (web::Data::new(config), key, model, account)
}

async fn enqueue(state: &web::Data<ServerState>, kind: &str, key: &str, model: &str) -> String {
    if kind == "image" {
        return super::super::enqueue_image_job(
            state,
            key,
            state.credential_cipher.encrypt(key).unwrap(),
            serde_json::from_value(serde_json::json!({"model":model,"prompt":"test"})).unwrap(),
        )
        .unwrap_or_else(|_| panic!("入队失败"));
    }
    let body = serde_json::json!({"model":model,"input":"test","async":true});
    let request = actix_web::test::TestRequest::post()
        .uri("/v1/responses")
        .to_http_request();
    let context = crate::request_identity::ProtocolRequest::from_http(&request, key, &body);
    let id = context.task_id.clone();
    assert_eq!(
        super::super::enqueue_text(state.clone(), key.into(), body, "responses", context)
            .await
            .status(),
        202
    );
    id
}

#[actix_web::test]
async fn worker_automatically_drains_both_queues_at_configured_account_concurrency() {
    for kind in ["image", "text"] {
        for limit in [1, 2] {
            let (state, key, model, account) = scheduling_state(kind, limit);
            let mut ids = Vec::new();
            for _ in 0..4 {
                ids.push(enqueue(&state, kind, key, model).await);
            }
            start_worker(state.clone());
            let mut peak = 0;
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let tasks = state.store.dispatch_tasks(None);
                    let active = tasks
                        .iter()
                        .filter(|task| {
                            task.account_id.as_deref() == Some(account)
                                && task.status == "processing"
                        })
                        .count();
                    assert!(active <= limit as usize, "{kind} exceeded {limit}");
                    peak = peak.max(active);
                    if ids.iter().all(|id| {
                        state.store.generation_job(id, None).unwrap().state
                            == GenerationJobState::Completed
                    }) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(peak, limit as usize);
            for id in ids {
                assert_eq!(
                    state.store.generation_job(&id, None).unwrap().retry_count,
                    0
                );
            }
        }
    }
}

#[actix_web::test]
async fn timed_quota_keeps_bound_tasks_queued_then_automatically_resumes_original_account() {
    for kind in ["image", "text"] {
        let (state, key, model, account) = scheduling_state(kind, 1);
        let id = enqueue(&state, kind, key, model).await;
        state.store.set_task_account(&id, account);
        let mut other = state.store.account_by_id(account).unwrap();
        other.id = "other".into();
        state.store.insert_account(other);
        state
            .store
            .mark_account_state_for_seconds(account, "daily_quota_exhausted", 1);
        state.store.record_account_failure(account, "quota", kind);
        start_worker(state.clone());
        tokio::time::sleep(Duration::from_millis(150)).await;
        let waiting = state.store.generation_job(&id, None).unwrap();
        assert_eq!(waiting.state, GenerationJobState::Queued);
        assert_eq!(waiting.retry_count, 0);
        wait_completed(&state, &id).await;
        assert_eq!(
            state.store.task_by_id(&id).unwrap().account_id.as_deref(),
            Some(account)
        );
    }
}

#[actix_web::test]
async fn synchronous_image_and_text_wait_for_busy_account_and_start_after_release() {
    use actix_web::{test, App};
    for kind in ["image", "text"] {
        let (state, key, model, account) = scheduling_state(kind, 1);
        let platform = state.store.account_by_id(account).unwrap().platform;
        let manifest = state.plugin_loader.get_plugin(&platform).unwrap();
        let (_, busy) =
            crate::protocol_task::reserve(&state, manifest, model, kind, key, "busy", None)
                .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let (uri, body) = if kind == "image" {
            (
                "/v1/images/generations",
                serde_json::json!({"model":model,"prompt":"test"}),
            )
        } else {
            (
                "/v1/responses",
                serde_json::json!({"model":model,"input":"test"}),
            )
        };
        let request = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(uri)
                .insert_header(("authorization", format!("Bearer {key}")))
                .set_json(body)
                .to_request(),
        );
        let release = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(state
                .store
                .list_tasks(None)
                .iter()
                .any(|task| task.id != "busy" && task.status == "queued"));
            state.store.complete_task("busy", serde_json::json!({}));
            drop(busy);
        };
        let (response, _) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(request, release)
        })
        .await
        .unwrap();
        assert_eq!(response.status(), 200);
        assert!(state
            .store
            .list_tasks(None)
            .iter()
            .all(|task| task.status == "completed"));
    }
}

#[actix_web::test]
async fn image_recovery_cannot_bypass_original_account_quota_or_concurrency() {
    let (state, key, model, account) = scheduling_state("image", 1);
    let id = enqueue(&state, "image", key, model).await;
    state.store.set_task_account(&id, account);
    let body = serde_json::from_value(serde_json::json!({"model":model,"prompt":"test"})).unwrap();
    state
        .store
        .mark_account_state_for_seconds(account, "cooldown", 60);
    let result =
        crate::image_api::run_queued_image_flow(state.clone(), body, key.into(), &id).await;
    assert_eq!(result.unwrap_err().1, "account_unavailable");
    state.store.mark_account_state(account, "available");
    let manifest = state.plugin_loader.get_plugin("fake").unwrap();
    let (_, _busy) =
        crate::protocol_task::reserve(&state, manifest, model, "image", key, "busy", None).unwrap();
    let body = serde_json::from_value(serde_json::json!({"model":model,"prompt":"test"})).unwrap();
    let result =
        crate::image_api::run_queued_image_flow(state.clone(), body, key.into(), &id).await;
    assert_eq!(result.unwrap_err().1, "account_unavailable");
}

#[actix_web::test]
async fn upstream_quota_with_reset_time_preserves_job_for_retry_instead_of_failing() {
    for kind in ["image", "text"] {
        let (state, key, model, account) = scheduling_state(kind, 1);
        let id = enqueue(&state, kind, key, model).await;
        let now = Utc::now();
        state
            .store
            .claim_next_generation_job(now, now + ChronoDuration::hours(6))
            .unwrap();
        state.store.set_task_account(&id, account);
        state
            .store
            .mark_account_state_for_seconds(account, "daily_quota_exhausted", 60);
        state.store.record_account_failure(account, "quota", kind);
        state.store.fail_task(&id, "insufficient_quota".into());
        finish_execution(
            &state,
            &id,
            None,
            Err((429, "insufficient_quota".into(), "quota".into())),
        )
        .await;
        let job = state.store.generation_job(&id, None).unwrap();
        assert_eq!(job.state, GenerationJobState::RetryWait);
        assert_eq!(
            job.last_failure_category.as_deref(),
            Some("account_limited")
        );
        assert_eq!(state.store.task_by_id(&id).unwrap().status, "pending");
        assert_eq!(
            state.store.task_by_id(&id).unwrap().account_id.as_deref(),
            Some(account)
        );
    }
}
