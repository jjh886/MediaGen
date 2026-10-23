use super::*;
use actix_web::{test, App};
use serde_json::json;

async fn wait_job(state: &ServerState, id: &str, expected: crate::store::GenerationJobState) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if state.store.generation_job(id, None).unwrap().state == expected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[actix_web::test]
async fn async_text_reuses_image_queue_results_and_owner_isolation_for_both_endpoints() {
    for (endpoint, body) in [
        (
            "responses",
            json!({"model":"text-model","input":"测试","async":true}),
        ),
        (
            "chat/completions",
            json!({"model":"text-model","messages":[{"role":"user","content":"测试"}],"async":true}),
        ),
    ] {
        let state = crate::protocol_api_tests::state();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let uri = format!("/v1/{endpoint}");
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(&uri)
                .insert_header(("authorization", "Bearer key-a"))
                .insert_header(("x-client-request-id", "same-turn"))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), 202);
        let accepted: Value = test::read_body_json(response).await;
        let id = accepted["data"]["taskId"].as_str().unwrap();
        let again: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::post()
                .uri(&uri)
                .insert_header(("authorization", "Bearer key-a"))
                .insert_header(("x-client-request-id", "same-turn"))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(again["data"]["taskId"], id);
        assert_eq!(state.store.list_tasks(None).len(), 1);
        assert_eq!(
            state.store.generation_job(id, None).unwrap().task_kind,
            "text"
        );
        let queued: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get()
                .uri("/api/v2/generation-queue?taskKind=text")
                .insert_header(("authorization", "Bearer key-a"))
                .to_request(),
        )
        .await;
        assert_eq!(queued["data"]["items"][0]["taskId"], id);
        assert_eq!(queued["data"]["items"][0]["taskKind"], "text");
        super::super::worker::dispatch_available(&state);
        wait_job(&state, id, crate::store::GenerationJobState::Completed).await;
        let detail_uri = format!("/api/v2/generation-tasks/{id}");
        let result: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get()
                .uri(&detail_uri)
                .insert_header(("authorization", "Bearer key-a"))
                .to_request(),
        )
        .await;
        assert_eq!(result["data"]["status"], "completed");
        let result = &result["data"]["result"];
        if endpoint == "responses" {
            assert_eq!(result["object"], "response");
            assert_eq!(result["output"][0]["content"][0]["text"], "你好");
        } else {
            assert_eq!(result["object"], "chat.completion");
            assert_eq!(result["choices"][0]["message"]["content"], "你好");
        }
        let denied = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&detail_uri)
                .insert_header(("authorization", "Bearer key-b"))
                .to_request(),
        )
        .await;
        assert_eq!(denied.status(), 404);
        assert_eq!(
            state
                .store
                .account_by_id("account")
                .unwrap()
                .policy
                .text_used_total,
            1
        );
    }
}

#[actix_web::test]
async fn sync_is_default_and_async_requires_boolean_without_streaming() {
    let state = crate::protocol_api_tests::state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    for flag in [
        None,
        Some(json!(false)),
        Some(json!("true")),
        Some(json!(null)),
    ] {
        let mut body = json!({"model":"text-model","input":"同步"});
        if let Some(flag) = flag {
            body["async"] = flag;
        }
        let expected = if body.get("async").is_some_and(|flag| !flag.is_boolean()) {
            400
        } else {
            200
        };
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .set_json(body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status().as_u16(), expected);
    }
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/v1/responses")
            .insert_header(("authorization", "Bearer key-a"))
            .set_json(json!({"model":"text-model","input":"test","async":true,"stream":true}))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(state.store.active_generation_job_count("text"), 0);
}

#[actix_web::test]
async fn account_config_changes_queue_capacity_without_fixed_ceiling() {
    let state = crate::protocol_api_tests::state();
    let manifest = state.plugin_loader.get_plugin("test").unwrap();
    let mut leases = Vec::new();
    for index in 0..3 {
        leases.push(
            crate::protocol_task::reserve(
                &state,
                manifest,
                "text-model",
                "text",
                "key-a",
                &format!("running-{index}"),
                None,
            )
            .unwrap()
            .1,
        );
    }
    assert!(!has_capacity(&state));
    state.store.update_account(
        "account",
        crate::store::AccountUpdate {
            text_concurrency_limit: Some(5),
            ..Default::default()
        },
    );
    for index in 3..5 {
        assert!(has_capacity(&state));
        leases.push(
            crate::protocol_task::reserve(
                &state,
                manifest,
                "text-model",
                "text",
                "key-b",
                &format!("running-{index}"),
                None,
            )
            .unwrap()
            .1,
        );
    }
    assert!(!has_capacity(&state));
    assert!(crate::protocol_task::reserve(
        &state,
        manifest,
        "text-model",
        "text",
        "key-a",
        "overflow",
        None
    )
    .is_err());
    drop(leases);
    state.store.update_account(
        "account",
        crate::store::AccountUpdate {
            text_concurrency_limit: Some(0),
            ..Default::default()
        },
    );
    assert_eq!(
        crate::store_helpers::text_concurrency_limit(
            &state.store.account_by_id("account").unwrap()
        ),
        3
    );
    assert!(has_capacity(&state));
}

#[actix_web::test]
async fn cancelled_async_text_never_starts_and_remains_owned() {
    let state = crate::protocol_api_tests::state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let accepted: Value = test::call_and_read_body_json(
        &app,
        test::TestRequest::post()
            .uri("/v1/responses")
            .insert_header(("authorization", "Bearer key-a"))
            .set_json(json!({"model":"text-model","input":"cancel","async":true}))
            .to_request(),
    )
    .await;
    let id = accepted["data"]["taskId"].as_str().unwrap();
    let action = json!({"action":"cancel","taskIds":[id]});
    let denied = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/v2/generation-queue/actions")
            .insert_header(("authorization", "Bearer key-b"))
            .set_json(&action)
            .to_request(),
    )
    .await;
    assert_ne!(denied.status(), 200);
    let cancelled = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/v2/generation-queue/actions")
            .insert_header(("authorization", "Bearer key-a"))
            .set_json(action)
            .to_request(),
    )
    .await;
    assert_eq!(cancelled.status(), 200);
    super::super::worker::dispatch_available(&state);
    assert_eq!(
        state.store.generation_job(id, None).unwrap().state,
        crate::store::GenerationJobState::Cancelled
    );
    assert_eq!(
        state
            .store
            .account_by_id("account")
            .unwrap()
            .policy
            .text_used_total,
        0
    );
}

#[actix_web::test]
async fn retry_after_reload_keeps_checkpoint_and_does_not_submit_again() {
    let original = crate::protocol_api_tests::state();
    let mut config = original.get_ref().clone();
    let root = config.store.path.parent().unwrap().to_owned();
    config.credential_cipher =
        std::sync::Arc::new(super::super::CredentialCipher::new(&root).unwrap());
    let directory = config.plugin_loader.get_plugin_dir("test").unwrap();
    std::fs::write(directory.join("scripts/text.js"), r#"function main(ctx) {
        const saved=host.checkpoint() || {};
        const path=ctx.profile_dir+'/submissions.json';
        if (!saved.conversation_id) {
            const old=host.tryCall('file.read_json',{path});
            host.call('file.write_json',{path,value:{count:(old.data?.count || 0)+1}});
            host.checkpoint({phase:'accepted',conversation_id:'original'});
            throw new Error('temporary_unavailable: retry original');
        }
        if(saved.conversation_id!=='original') throw new Error('lost checkpoint');
        return {response:{id:'resp_original',object:'response',status:'completed',output:[{type:'message',role:'assistant',content:[{type:'output_text',text:'恢复原结果'}]}]}};
    }"#).unwrap();
    let state = web::Data::new(config.clone());
    let body = json!({"model":"text-model","input":"恢复","async":true,"store":false});
    let request = test::TestRequest::post()
        .uri("/v1/responses")
        .insert_header(("x-client-request-id", "restart"))
        .to_http_request();
    let context = ProtocolRequest::from_http(&request, "key-a", &body);
    let id = context.task_id.clone();
    assert_eq!(
        enqueue_text(state.clone(), "key-a".into(), body, "responses", context)
            .await
            .status(),
        202
    );
    super::super::worker::dispatch_available(&state);
    wait_job(&state, &id, crate::store::GenerationJobState::RetryWait).await;
    assert_eq!(
        state.store.generation_job(&id, None).unwrap().retry_count,
        1
    );
    // 模拟下一次进程启动；任务、密钥和检查点都重新读取。
    config.store =
        std::sync::Arc::new(crate::store::AppStore::load(root.join("server-state.json")));
    config.credential_cipher =
        std::sync::Arc::new(super::super::CredentialCipher::new(&root).unwrap());
    config.store.generation_jobs.lock().unwrap()[0].next_attempt_at =
        chrono::Utc::now().to_rfc3339();
    let restarted = web::Data::new(config);
    super::super::worker::dispatch_available(&restarted);
    wait_job(&restarted, &id, crate::store::GenerationJobState::Completed).await;
    let result = super::super::result::read(&restarted, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["output"][0]["content"][0]["text"], "恢复原结果");
    let account = restarted.store.account_by_id("account").unwrap();
    let count: Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(&account.profile_dir).join("submissions.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(count["count"], 1);
    assert_eq!(account.policy.text_used_total, 1);
    assert_eq!(restarted.store.list_tasks(None).len(), 1);
}

#[actix_web::test]
async fn admin_reads_completed_text_without_exposing_result_through_public_queue() {
    let state = crate::protocol_api_tests::state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .wrap_fn(|req, service| {
                use actix_web::dev::Service;
                use futures_util::future::{ready, Either};
                if crate::admin_access::request_allowed(&req) {
                    Either::Left(service.call(req))
                } else {
                    Either::Right(ready(Ok(
                        req.into_response(crate::admin_access::unauthorized_response())
                    )))
                }
            })
            .configure(crate::configure_routes),
    )
    .await;
    let request = test::TestRequest::post()
        .uri("/v1/responses")
        .to_http_request();
    let body = json!({"model":"text-model","input":"test","async":true,"store":false});
    let context = ProtocolRequest::from_http(&request, "key-a", &body);
    let id = context.task_id.clone();
    enqueue_text(state.clone(), "key-a".into(), body, "responses", context).await;
    super::super::worker::dispatch_available(&state);
    wait_job(&state, &id, crate::store::GenerationJobState::Completed).await;
    let uri = format!("/api/tasks/{id}/result");
    let denied = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&uri)
            .peer_addr("203.0.113.1:1234".parse().unwrap())
            .insert_header(("authorization", "Bearer key-a"))
            .to_request(),
    )
    .await;
    assert_eq!(denied.status(), 401);
    let response = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&uri)
            .peer_addr("127.0.0.1:1234".parse().unwrap())
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let result: Value = test::read_body_json(response).await;
    assert_eq!(result["data"]["output"][0]["content"][0]["text"], "你好");
    assert!(result["data"].get("owner").is_none());
}
