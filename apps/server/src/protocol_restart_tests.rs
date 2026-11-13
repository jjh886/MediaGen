use actix_web::{test, web, App};
use serde_json::{json, Value};
use std::sync::Arc;

#[actix_web::test]
async fn rate_limited_new_turn_can_failover_but_bound_conversation_keeps_account() {
    for (continued, available) in [(false, true), (false, false), (true, true)] {
        for (failure, category) in [
            ("rate_limit_exceeded: HTTP 429", "account_rate_limited"),
            (
                "client error (ProxyConnect): unexpected end of file",
                "proxy_unavailable",
            ),
            (
                "account_model_unavailable: permission denied",
                "account_model_unavailable",
            ),
            (
                "account_model_unavailable: model_quota_exhausted; resets_at=2099-11-10T16:51:45Z",
                "account_model_unavailable",
            ),
        ] {
            let state = crate::protocol_api_tests::state();
            let root = state.store.path.parent().unwrap().to_owned();
            let mut profiles = Vec::new();
            for id in ["second", "third", "fourth"] {
                let candidate = crate::test_fixtures::account_record(id, "test", id, "");
                profiles.push(candidate.profile_dir.clone());
                state.store.insert_account(candidate);
            }
            let mut account = state.store.account_by_id("account").unwrap();
            let first_profile = account.profile_dir.clone();
            let directory = state.plugin_loader.get_plugin_dir("test").unwrap();
            std::fs::write(
                directory.join("scripts/text.js"),
                r#"function main(ctx) {
            host.checkpoint({phase:'ready'});
            if (ctx.account_id !== TEST_AVAILABLE) throw new Error(TEST_FAILURE);
            return {response:{output_text:'OK'}};
        }"#
                .replace("TEST_FAILURE", &serde_json::to_string(failure).unwrap())
                .replace(
                    "TEST_AVAILABLE",
                    if available { "'fourth'" } else { "null" },
                ),
            )
            .unwrap();
            let (manifest, _) = state.plugin_loader.resolve_model("text-model").unwrap();
            let mut manifest = manifest.clone();
            manifest.extra.insert(
                "failure_strategy".into(),
                json!([{
                    "contains_any":[failure], "category":category,
                    "retry_action":"failover", "usage_consumed":false
                }]),
            );
            let mut request = json!({"model":"text-model","input":"继续"});
            if continued {
                request["options"] = json!({"platform_session":{"conversation_id":"original"}});
            }
            let (events, _receiver) = tokio::sync::mpsc::channel(4);
            let result = crate::protocol_task::run_with_failover(
                &state,
                &manifest,
                &mut account,
                "generate_text",
                request,
                Some(events),
                "rate-limit-test",
                Value::Null,
                "key-a",
            )
            .await;
            if continued || !available {
                let error = result.unwrap_err();
                if !continued
                    && category == "account_model_unavailable"
                    && !failure.contains("model_quota_exhausted")
                {
                    assert!(
                        error.starts_with("no_available_account_for_model:"),
                        "{error}"
                    );
                    assert!(error.contains("text-model"));
                } else {
                    assert!(error.contains(failure), "{error}");
                }
                assert_eq!(account.id, if continued { "account" } else { "fourth" });
            } else {
                assert_eq!(result.unwrap()["response"]["output_text"], "OK");
                assert_eq!(account.id, "fourth");
            }
            let logs = state.store.list_audit_logs();
            assert_eq!(logs.len(), if continued { 0 } else { 3 });
            assert!(logs.iter().all(|log| log.detail.contains(failure)));
            drop(state);
            let _ = std::fs::remove_dir_all(root);
            let _ = std::fs::remove_dir_all(first_profile);
            for profile in profiles {
                let _ = std::fs::remove_dir_all(profile);
            }
        }
    }
}

#[actix_web::test]
async fn interrupted_protocol_reopens_store_and_cipher_then_queries_once_in_both_stream_modes() {
    for stream in [false, true] {
        let initial = crate::protocol_api_tests::state();
        let root = initial.store.path.parent().unwrap().to_owned();
        let mut configured = initial.get_ref().clone();
        configured.credential_cipher =
            Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
        let directory = configured.plugin_loader.get_plugin_dir("test").unwrap();
        std::fs::write(directory.join("scripts/text.js"), r#"function main(ctx) {
            const saved = host.checkpoint() || {};
            const path = ctx.profile_dir + '/submissions.json';
            if (!saved.conversation_id) {
                const old = host.tryCall('file.read_json', {path});
                host.call('file.write_json', {path, value:{count:(old.data?.count || 0)+1}});
                host.checkpoint({phase:'accepted',conversation_id:'original'});
                throw new Error('temporary_unavailable: test interrupted after acceptance');
            }
            if(saved.conversation_id!=='original') throw new Error('wrong conversation');
            const response={id:'resp_original',object:'response',status:'completed',model:ctx.request.model,output:[{id:'msg_a',type:'message',role:'assistant',status:'completed',content:[{type:'output_text',text:'原会话结果',annotations:[]}]}]};
            host.emit({type:'response.created',response});
            host.emit({type:'response.output_text.delta',item_id:'msg_a',delta:'原会话结果'});
            host.emit({type:'response.completed',response});
            return {success:true,response};
        }"#).unwrap();
        let body = json!({"model":"text-model","input":"原请求","stream":stream});
        let mut task_id = String::new();
        for pass in 0..3 {
            // 每次重新加载持久化存储与密钥，旧执行器和流对象不能参与恢复。
            if pass > 0 {
                configured.store =
                    Arc::new(crate::store::AppStore::load(root.join("server-state.json")));
                configured.credential_cipher =
                    Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
            }
            let app = test::init_service(
                App::new()
                    .app_data(web::Data::new(configured.clone()))
                    .configure(crate::configure_routes),
            )
            .await;
            let response = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/v1/responses")
                    .insert_header(("authorization", "Bearer key-a"))
                    .insert_header(("x-client-request-id", "original-client-request"))
                    .set_json(&body)
                    .to_request(),
            )
            .await;
            assert_eq!(
                response.status().as_u16(),
                if pass == 0 { 503 } else { 200 }
            );
            let bytes = test::read_body(response).await;
            if pass > 0 {
                assert!(String::from_utf8_lossy(&bytes).contains("原会话结果"));
            }
            let tasks = configured.store.list_tasks(None);
            assert_eq!(tasks.len(), 1);
            if pass == 0 {
                task_id = tasks[0].id.clone();
            }
            assert_eq!(tasks[0].id, task_id);
            assert_eq!(tasks[0].account_id.as_deref(), Some("account"));
            let profile = configured
                .store
                .account_by_id("account")
                .unwrap()
                .profile_dir;
            let counter: Value = serde_json::from_slice(
                &std::fs::read(std::path::Path::new(&profile).join("submissions.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(counter["count"], 1);
        }
        assert_eq!(
            configured
                .store
                .account_by_id("account")
                .unwrap()
                .policy
                .text_used_total,
            1
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[actix_web::test]
async fn sync_image_request_id_recovers_checkpoint_and_replays_result_without_regeneration() {
    let mut state = crate::image_api_tests::queue_state("image-sync-recovery");
    let directory = state.plugin_loader.get_plugin_dir("fake").unwrap();
    let path = directory.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let script = manifest["flows"]["generate_image"]["steps"][0]["script"]
        .as_str()
        .unwrap();
    manifest["flows"]["generate_image"]["steps"][0]["script"] = json!(script.replacen(
        "function main(ctx) {", "function main(ctx) { const saved=host.checkpoint(); if(!saved.phase){host.checkpoint({phase:'accepted',upstream:'one'});throw new Error('temporary_unavailable: still running')} if(saved.upstream!=='one')throw new Error('lost checkpoint');", 1));
    std::fs::write(&path, manifest.to_string()).unwrap();
    state.plugin_loader = std::sync::Arc::new(
        mediagen_plugin_runtime::PluginLoader::load_from(directory.parent().unwrap()).unwrap(),
    );
    let mut account = state.store.account_by_id("acc-queue").unwrap();
    account.profile_dir = state.file_root.join("profile").to_string_lossy().into();
    state.store.delete_account("acc-queue");
    state.store.insert_account(account);
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let first = test::call_service(
        &app,
        crate::image_api_tests::image_request()
            .insert_header(("x-client-request-id", "original-image"))
            .to_request(),
    )
    .await;
    assert_eq!(first.status(), 503);
    let task = state.store.list_tasks(None)[0].clone();
    assert_eq!(task.status, "failed");
    assert_ne!(task.status, "cancelled");
    let next = test::call_service(
        &app,
        crate::image_api_tests::image_request()
            .insert_header(("x-client-request-id", "original-image"))
            .to_request(),
    )
    .await;
    assert_eq!(next.status(), 200);
    let next: serde_json::Value = test::read_body_json(next).await;
    // 结果已保存后，账号不可用也只能回放原结果，不能重新调度。
    state.store.mark_account_state("acc-queue", "login_expired");
    let replay = test::call_service(
        &app,
        crate::image_api_tests::image_request()
            .insert_header(("x-client-request-id", "original-image"))
            .to_request(),
    )
    .await;
    assert_eq!(replay.status(), 200);
    let replay: serde_json::Value = test::read_body_json(replay).await;
    assert_eq!(next, replay);
    assert_eq!(
        state.store.account_by_id("acc-queue").unwrap().used_total,
        1
    );
    assert!(state
        .store
        .task_by_id(&task.id)
        .unwrap()
        .error_message
        .is_none());
    assert_eq!(state.store.list_tasks(None).len(), 1);
    assert_eq!(
        state.store.task_by_id(&task.id).unwrap().account_id,
        task.account_id
    );
}

// 显式运行的真实接口验证；使用临时资料副本，避免覆盖线上登录和任务检查点。
#[actix_web::test]
#[ignore = "需要 MEDIAGEN_LIVE_DATA 和 MEDIAGEN_LIVE_PROFILE，实际消耗一次生成额度"]
async fn live_gemini_generation_and_history_recovery() {
    use sha2::{Digest, Sha256};
    let data = std::env::var("MEDIAGEN_LIVE_DATA").expect("live data directory");
    let original = std::env::var("MEDIAGEN_LIVE_PROFILE").expect("live account profile");
    let model = std::env::var("MEDIAGEN_LIVE_MODEL").unwrap_or_else(|_| "gemini-4-argon".into());
    let old_task = std::env::var("MEDIAGEN_LIVE_TASK").ok();
    let task = old_task
        .clone()
        .unwrap_or_else(|| format!("probe-{}", uuid::Uuid::new_v4()));
    let temporary = tempfile::tempdir().unwrap();
    let profile = temporary.path().join("profile");
    std::fs::create_dir_all(profile.join("checkpoints")).unwrap();
    std::fs::copy(
        std::path::Path::new(&original).join("auth.json"),
        profile.join("auth.json"),
    )
    .unwrap();
    if old_task.is_some() {
        let file = format!("{:x}.json", Sha256::digest(task.as_bytes()));
        std::fs::copy(
            std::path::Path::new(&original)
                .join("checkpoints")
                .join(&file),
            profile.join("checkpoints").join(file),
        )
        .unwrap();
    }
    let plugins = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let mut state = crate::test_fixtures::server_state(
        temporary.path().join("state"),
        mediagen_plugin_runtime::PluginLoader::load_from(&plugins).unwrap(),
    );
    state.credential_cipher = Arc::new(
        crate::generation_queue::CredentialCipher::new(std::path::Path::new(&data)).unwrap(),
    );
    let mut account = crate::test_fixtures::account_record("probe", "gemini", "probe", "");
    account.profile_dir = profile.to_string_lossy().into_owned();
    account.browser_os = "linux".into();
    account.browser_version = "154".into();
    let manifest = state.plugin_loader.get_plugin("gemini").unwrap();
    let request = json!({"model":model,"input":"Reply only LIVE_GEMINI_OK","store":false,"options":{"session_key":"live-probe"}});
    let first = crate::protocol_task::run_plugin(
        &state,
        manifest,
        &account,
        "generate_text",
        request.clone(),
        None,
        &task,
        Value::Null,
    )
    .await;
    if old_task.is_some() {
        let error = first.unwrap_err();
        assert!(
            error.contains("conversation_unavailable"),
            "{}",
            crate::protocol_error::sanitize_error(&error)
        );
        let start = std::time::Instant::now();
        let again = crate::protocol_task::run_plugin(
            &state,
            manifest,
            &account,
            "generate_text",
            request,
            None,
            &task,
            Value::Null,
        )
        .await
        .unwrap_err();
        assert!(again.contains("conversation_unavailable"));
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        println!("LIVE_RESULT: unavailable conversation is terminal and repeated recovery makes no upstream request");
        return;
    }
    let result =
        first.unwrap_or_else(|error| panic!("{}", crate::protocol_error::sanitize_error(&error)));
    assert!(result["response"]["output_text"]
        .as_str()
        .is_some_and(|text| text.contains("LIVE_GEMINI_OK")));
    println!(
        "LIVE_RESPONSE: {}",
        json!({"requested":model,"actual":result["response"]["model"]})
    );
    assert!(!result["response"]["output_text"]
        .as_str()
        .unwrap()
        .contains("已改用"));
    for key in [
        "requested_model",
        "actual_model",
        "actual_model_name",
        "model_notice",
    ] {
        assert!(result["response"]["metadata"].get(key).is_none());
    }
    let host = crate::server_plugin_host::ServerPluginHost::new_for_account(&account)
        .with_storage(
            state.credential_cipher.clone(),
            &account.profile_dir,
            manifest,
        )
        .with_events(None, task.clone());
    let mut saved = result["response"]["metadata"]["platform_session"].clone();
    assert!(saved["conversation_id"].is_string());
    assert!(saved["response_id"].is_string());
    saved["phase"] = json!("accepted");
    saved["accepted_at"] = json!(chrono::Utc::now().timestamp_millis());
    host.checkpoint(&json!({"value":saved})).await.unwrap();
    let recovered = crate::protocol_task::run_plugin(
        &state,
        manifest,
        &account,
        "generate_text",
        request,
        None,
        &task,
        Value::Null,
    )
    .await
    .unwrap_or_else(|error| panic!("{}", crate::protocol_error::sanitize_error(&error)));
    assert_eq!(
        result["response"]["output_text"],
        recovered["response"]["output_text"]
    );
    println!("LIVE_RESULT: {model} generation and original-turn history recovery both succeeded");
}

#[actix_web::test]
async fn repeated_transport_failure_keeps_receipt_and_stops_before_fourth_attempt() {
    let state = crate::protocol_api_tests::state();
    let account = state.store.account_by_id("account").unwrap();
    let manifest = state.plugin_loader.get_plugin("test").unwrap();
    let script = state
        .plugin_loader
        .get_plugin_dir("test")
        .unwrap()
        .join("scripts/text.js");
    std::fs::write(&script, "function main(){const saved=host.checkpoint();host.checkpoint({...saved,phase:'accepted',conversation_id:'original'});throw new Error('operation timed out');}").unwrap();
    for attempt in 1..=4 {
        if attempt == 4 {
            std::fs::write(&script, "function main(){throw new Error('must not run')}").unwrap();
        }
        let error = crate::protocol_task::run_plugin(
            &state,
            manifest,
            &account,
            "generate_text",
            json!({"model":"text-model","input":"test"}),
            None,
            "bounded-transport",
            Value::Null,
        )
        .await
        .unwrap_err();
        assert!(error.contains(if attempt >= 3 {
            "recovery_exhausted"
        } else {
            "operation timed out"
        }));
        assert!(!error.contains("must not run"));
    }
    let host = crate::server_plugin_host::ServerPluginHost::new_for_account(&account)
        .with_storage(
            state.credential_cipher.clone(),
            &account.profile_dir,
            manifest,
        )
        .with_events(None, "bounded-transport".into());
    let saved = host.checkpoint(&json!({})).await.unwrap();
    assert_eq!(saved["conversation_id"], "original");
    assert_eq!(saved["_transport_failures"], 3);
}

#[path = "protocol_session_tests.rs"]
mod session_tests;
