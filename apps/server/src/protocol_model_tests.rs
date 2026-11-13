use super::*;
use crate::test_fixtures::account_record;
use mediagen_plugin_runtime::PluginLoader;

#[actix_web::test]
async fn dynamic_model_reservation_uses_other_free_accounts() {
    let state = crate::test_fixtures::protocol_state(true);
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
    let state = crate::test_fixtures::protocol_state(true);
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

#[actix_web::test]
async fn discovered_models_keep_plugin_prices_and_authoritative_capabilities() {
    let mut state = state().into_inner();
    let inner = std::sync::Arc::get_mut(&mut state).unwrap();
    let path = inner.store.path.with_file_name("model-catalog.json");
    std::fs::write(&path, json!({"account":{"models":[{"model_id":"text-model","display_name":"Live","task_kind":"text","capabilities":{"input_modalities":["text","image"],"streaming":false,"context_window":300000,"reasoning":{"efforts":["low"],"default_effort":"low"}}}],"refreshed_at":chrono::Utc::now().timestamp()}}).to_string()).unwrap();
    inner.model_catalog = crate::model_api::Catalog::load(&path);
    let plugin_dir = inner
        .plugin_loader
        .get_plugin_dir("test")
        .unwrap()
        .to_path_buf();
    let manifest_path = plugin_dir.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["models"][0]["capabilities"]["pricing"] =
        json!({"input":2,"output":10,"reference_only":true});
    manifest["models"][0]["capabilities"]["context_window"] = json!(1050000);
    manifest["models"][0]["capabilities"]["reasoning"] =
        json!({"efforts":["low","high"],"default_effort":"high"});
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    inner.plugin_loader = PluginLoader::load_from(plugin_dir.parent().unwrap())
        .unwrap()
        .into();
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::from(state))
            .configure(crate::configure_routes),
    )
    .await;
    for uri in ["/v1/models/text-model", "/api/accounts/account/models"] {
        let body: Value = actix_test::call_and_read_body_json(
            &app,
            actix_test::TestRequest::get()
                .uri(uri)
                .insert_header(("authorization", "Bearer key-a"))
                .to_request(),
        )
        .await;
        let model = if uri.starts_with("/v1") {
            &body
        } else {
            body["data"]["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|model| model["id"] == "text-model")
                .unwrap()
        };
        assert_eq!(model["capabilities"]["pricing"]["output"], 10);
        assert_eq!(model["capabilities"]["context_window"], 300000);
        assert_eq!(model["capabilities"]["streaming"], false);
        assert_eq!(
            model["capabilities"]["reasoning"]["efforts"],
            json!(["low"])
        );
        assert_eq!(model["capabilities"]["reasoning"]["default_effort"], "low");
    }
}
