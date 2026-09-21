use crate::{
    test_fixtures::{account_record, server_state},
    ServerState,
};
use actix_web::{test as actix_test, web, App};
use mediagen_plugin_runtime::PluginLoader;
use serde_json::{json, Value};

pub(crate) fn state() -> web::Data<ServerState> {
    let root = std::env::temp_dir().join(format!("protocol-test-{}", uuid::Uuid::new_v4()));
    let dir = root.join("plugins/test/scripts");
    std::fs::create_dir_all(&dir).unwrap();
    let script = r#"function main(ctx) {
        const response={id:'resp_'+ctx.request_id,object:'response',created_at:1,status:'completed',model:ctx.request.model,output:[{id:'msg_a',type:'message',role:'assistant',status:'completed',content:[{type:'output_text',text:'你好',annotations:[]}]}],usage:{input_tokens:2,output_tokens:3,total_tokens:5}};
        host.emit({type:'response.created',sequence_number:0,response:{...response,status:'in_progress',output:[]}});
        host.emit({type:'response.output_text.delta',sequence_number:1,item_id:'msg_a',output_index:0,content_index:0,delta:'你好'});
        host.emit({type:'response.completed',sequence_number:2,response});
        return {response};
    }"#;
    std::fs::write(dir.join("text.js"), script).unwrap();
    std::fs::write(dir.join("models.js"),"function main(){return {models:[{model_id:'new-model',display_name:'New',task_kind:'text',capabilities:{input_modalities:['text']}}]}}").unwrap();
    std::fs::write(
        dir.join("speech.js"),
        "function main(){return {audio_base64:'SUQz',mime_type:'audio/mpeg'}}",
    )
    .unwrap();
    std::fs::write(root.join("plugins/test/manifest.json"),json!({"platform_id":"test","display_name":"Test","adapter_mode":"api","models":[{"model_id":"text-model","display_name":"Text","task_kind":"text","capabilities":{"input_modalities":["text"],"streaming":true}},{"model_id":"speech-model","display_name":"Speech","task_kind":"audio_speech"}],"flows":{"generate_text":{"steps":[{"action":"plugin.run_js","script_file":"text.js"}]},"generate_audio":{"steps":[{"action":"plugin.run_js","script_file":"speech.js"}]},"list_models":{"steps":[{"action":"plugin.run_js","script_file":"models.js"}]}}}).to_string()).unwrap();
    let loader = PluginLoader::load_from(&root.join("plugins")).unwrap();
    let state = server_state(root, loader);
    state.store.update_settings(crate::store::SettingsUpdate {
        api_keys: Some("key-a\nkey-b".into()),
        ..Default::default()
    });
    state
        .store
        .insert_account(account_record("account", "test", "Account", ""));
    web::Data::new(state)
}

#[actix_web::test]
async fn responses_storage_is_owned_and_deletion_removes_retrieval() {
    let state = state();
    let app = actix_test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/responses")
            .insert_header(("authorization", "Bearer key-a"))
            .set_json(json!({"model":"text-model","input":"测试"}))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(body["store"], true);
    let uri = format!("/v1/responses/{}", body["id"].as_str().unwrap());
    let denied = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri(&uri)
            .insert_header(("authorization", "Bearer key-b"))
            .to_request(),
    )
    .await;
    assert_eq!(denied.status(), 404);
    let input: Value = actix_test::call_and_read_body_json(
        &app,
        actix_test::TestRequest::get()
            .uri(&(uri.clone() + "/input_items"))
            .insert_header(("authorization", "Bearer key-a"))
            .to_request(),
    )
    .await;
    assert_eq!(input["data"][0]["content"][0]["text"], "测试");
    let next: Value = actix_test::call_and_read_body_json(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/responses")
            .insert_header(("authorization", "Bearer key-a"))
            .set_json(
                json!({"model":"text-model","input":"继续","previous_response_id":body["id"]}),
            )
            .to_request(),
    )
    .await;
    assert_eq!(next["previous_response_id"], body["id"]);
    let deleted = actix_test::call_service(
        &app,
        actix_test::TestRequest::delete()
            .uri(&uri)
            .insert_header(("authorization", "Bearer key-a"))
            .to_request(),
    )
    .await;
    assert_eq!(deleted.status(), 200);
    let missing = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri(&uri)
            .insert_header(("authorization", "Bearer key-a"))
            .to_request(),
    )
    .await;
    assert_eq!(missing.status(), 404);
}

#[actix_web::test]
async fn chat_stream_has_usage_then_done_and_releases_account() {
    let state = state();
    let app = actix_test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let response=actix_test::call_service(&app,actix_test::TestRequest::post().uri("/v1/chat/completions").insert_header(("authorization","Bearer key-a")).set_json(json!({"model":"text-model","messages":[{"role":"user","content":"测试"}],"stream":true,"stream_options":{"include_usage":true}})).to_request()).await;
    assert_eq!(response.status(), 200);
    let body = String::from_utf8(actix_test::read_body(response).await.to_vec()).unwrap();
    assert!(body.contains("你好"));
    assert!(body.contains("\"prompt_tokens\":2"));
    assert!(body.ends_with("data: [DONE]\n\n"));
    assert_eq!(body.matches("[DONE]").count(), 1);
    assert_eq!(state.store.list_tasks(None)[0].status, "completed");
    assert!(state
        .store
        .first_available_account_for_key("test", Some("key-a"))
        .is_some());
}

#[actix_web::test]
async fn discovery_models_are_callable_and_speech_returns_binary() {
    let state = state();
    let app = actix_test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let models: Value = actix_test::call_and_read_body_json(
        &app,
        actix_test::TestRequest::get()
            .uri("/v1/models?refresh=true")
            .insert_header(("authorization", "Bearer key-a"))
            .to_request(),
    )
    .await;
    assert!(models["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["id"] == "new-model"));
    let generated = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/responses")
            .insert_header(("authorization", "Bearer key-a"))
            .set_json(json!({"model":"new-model","input":"hi","store":false}))
            .to_request(),
    )
    .await;
    assert_eq!(generated.status(), 200);
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/audio/speech")
            .insert_header(("authorization", "Bearer key-a"))
            .set_json(json!({"model":"speech-model","input":"hi","voice":"alloy"}))
            .to_request(),
    )
    .await;
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "audio/mpeg"
    );
    assert_eq!(actix_test::read_body(response).await.as_ref(), b"ID3");
}

#[actix_web::test]
async fn invalid_requests_return_openai_errors_and_request_id() {
    let state = state();
    let app = actix_test::init_service(
        App::new()
            .app_data(state)
            .wrap(actix_web::middleware::from_fn(
                crate::request_identity::middleware,
            ))
            .configure(crate::configure_routes),
    )
    .await;
    for (uri, body, expected) in [
        (
            "/v1/chat/completions",
            json!({"model":"text-model","messages":[]}),
            400,
        ),
        (
            "/v1/audio/speech",
            json!({"model":"text-model","input":"hi","voice":"alloy"}),
            400,
        ),
        (
            "/v1/responses",
            json!({"model":"missing","input":"hi"}),
            404,
        ),
    ] {
        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::post()
                .uri(uri)
                .insert_header(("authorization", "Bearer key-a"))
                .set_json(body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status().as_u16(), expected);
        assert!(response.headers().contains_key("x-request-id"));
        let body: Value = actix_test::read_body_json(response).await;
        assert!(body["error"]["code"].is_string());
        assert!(body["error"].get("param").is_some());
    }
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/v1/realtime?call_id=other")
            .insert_header(("authorization", "Bearer key-a"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 404);
}

#[test]
fn chat_conversion_preserves_images_calls_and_tool_outputs() {
    let value=crate::protocol_request::chat_to_responses(&json!({"model":"text-model","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,YQ=="}}]},{"role":"assistant","content":null,"tool_calls":[{"id":"call_a","type":"function","function":{"name":"weather","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_a","content":"sunny"}]})).unwrap();
    assert_eq!(value["input"][0]["content"][0]["type"], "input_image");
    assert_eq!(value["input"][1]["type"], "function_call");
    assert_eq!(value["input"][2]["call_id"], "call_a");
}

#[actix_web::test]
async fn dynamic_model_reservation_uses_other_free_accounts() {
    let state = state();
    state.store.update_account(
        "account",
        crate::store::AccountUpdate {
            text_concurrency_limit: Some(1),
            ..Default::default()
        },
    );
    let mut second = account_record("second", "test", "Second", "");
    second.policy.text_concurrency_limit = 1;
    state.store.insert_account(second);
    for account in state.store.list_accounts(None) {
        crate::model_api::refresh_account(&state, account)
            .await
            .unwrap();
    }
    let (manifest, model, _) = crate::model_api::resolve(&state, "new-model", "key-a").unwrap();
    let (first, lease1) = crate::protocol_task::reserve(
        &state,
        &manifest,
        &model.model_id,
        "text",
        "key-a",
        "turn1",
        None,
    )
    .unwrap();
    let (second, _lease2) = crate::protocol_task::reserve(
        &state,
        &manifest,
        &model.model_id,
        "text",
        "key-a",
        "turn2",
        None,
    )
    .unwrap();
    assert_ne!(first.id, second.id);
    assert!(crate::protocol_task::reserve(
        &state,
        &manifest,
        &model.model_id,
        "text",
        "key-a",
        "turn3",
        None
    )
    .is_err());
    state.store.cancel_task("turn1");
    state.store.complete_task("turn1", json!({}));
    state.store.fail_task("turn1", "late failure".into());
    assert_eq!(state.store.task_by_id("turn1").unwrap().status, "cancelled");
    assert!(crate::protocol_task::reserve(
        &state,
        &manifest,
        &model.model_id,
        "text",
        "key-a",
        "turn4",
        None
    )
    .is_err());
    drop(lease1);
    assert!(crate::protocol_task::reserve(
        &state,
        &manifest,
        &model.model_id,
        "text",
        "key-a",
        "turn5",
        None
    )
    .is_ok());
    let restored =
        crate::model_api::Catalog::load(&state.store.path.with_file_name("model-catalog.json"));
    assert_eq!(restored.0.lock().unwrap().len(), 2);
}

#[actix_web::test]
async fn discovery_failure_preserves_other_accounts_and_reports_partial_success() {
    let state = state();
    let mut broken = account_record("broken", "test", "Broken", "");
    broken.proxy_id = "deleted-proxy".into();
    state.store.insert_account(broken);
    let app = actix_test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/v1/models?refresh=true")
            .insert_header(("authorization", "Bearer key-a"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(body["refresh_errors"][0]["code"], "proxy_unavailable");
    assert!(body["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["id"] == "new-model"));
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/api/accounts/account/models?refresh=true")
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(body["data"]["discovery_supported"], true);
}

#[actix_web::test]
async fn pre_submission_failure_switches_account_but_uncertain_submission_never_replays() {
    for uncertain in [false, true] {
        let state = state();
        let directory = state.plugin_loader.get_plugin_dir("test").unwrap();
        let script_path = directory.join("scripts/text.js");
        let script = std::fs::read_to_string(&script_path).unwrap();
        let receipt = if uncertain {
            "host.checkpoint({phase:'submitting'});"
        } else {
            ""
        };
        let prefix = format!("function main(ctx) {{if(ctx.account_id==='account'){{{receipt}throw new Error('LOGIN_EXPIRED');}}");
        std::fs::write(
            &script_path,
            script.replacen("function main(ctx) {", &prefix, 1),
        )
        .unwrap();
        let manifest_path = directory.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest["failure_strategy"] = json!([{"contains_any":["LOGIN_EXPIRED"],"category":"login_expired","retry_action":"failover","account_state":"login_expired","usage_consumed":false}]);
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        let mut configured = state.get_ref().clone();
        configured.plugin_loader =
            std::sync::Arc::new(PluginLoader::load_from(directory.parent().unwrap()).unwrap());
        let state = web::Data::new(configured);
        state
            .store
            .insert_account(account_record("second", "test", "Second", ""));
        let app = actix_test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .set_json(json!({"model":"text-model","input":"hello"}))
                .to_request(),
        )
        .await;
        assert_eq!(
            response.status().as_u16(),
            if uncertain { 503 } else { 200 }
        );
        assert_eq!(
            state.store.account_by_id("account").unwrap().login_state,
            "login_expired"
        );
        assert_eq!(
            state
                .store
                .account_by_id("second")
                .unwrap()
                .policy
                .text_used_total,
            if uncertain { 0 } else { 1 }
        );
        assert_eq!(
            state.store.list_tasks(None)[0].account_id.as_deref(),
            Some(if uncertain { "account" } else { "second" })
        );
    }
}
