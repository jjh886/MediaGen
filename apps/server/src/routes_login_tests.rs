use actix_web::test as actix_test;
use mediagen_plugin_runtime::PluginLoader;
use serde_json::json;

use crate::store::TaskRecord;
use crate::test_fixtures::{account_record, server_state};

#[actix_web::test]
async fn inconclusive_login_check_keeps_the_existing_account_state() {
    let state = test_state("inconclusive");
    state.store.insert_account(account_record(
        "acc-login",
        "temporarily-unavailable-plugin",
        "登录账号",
        "",
    ));
    let app = crate::init_admin_test_app!(state.clone());

    let request = actix_test::TestRequest::post()
        .uri("/api/accounts/check-login")
        .set_json(json!({}))
        .to_request();
    let body: serde_json::Value = actix_test::call_and_read_body_json(&app, request).await;

    assert_eq!(body["checked"], 1);
    assert_eq!(body["expired"], 0);
    assert_eq!(body["inconclusive"], 1);
    assert_eq!(account_state(&state, "acc-login"), "available");
}

#[actix_web::test]
async fn active_task_skips_login_check_without_opening_another_browser() {
    let state = test_state("active-task");
    state
        .store
        .insert_account(account_record("acc-busy", "platform-a", "运行中账号", ""));
    state.store.insert_task(active_task("acc-busy"));
    let app = crate::init_admin_test_app!(state.clone());

    let request = actix_test::TestRequest::post()
        .uri("/api/accounts/check-login")
        .set_json(json!({}))
        .to_request();
    let body: serde_json::Value = actix_test::call_and_read_body_json(&app, request).await;

    assert_eq!(body["checked"], 0);
    assert_eq!(body["expired"], 0);
    assert_eq!(body["skipped_busy"], 1);
    assert_eq!(account_state(&state, "acc-busy"), "available");
    assert!(state.browser_gateway.list_sessions().unwrap().is_empty());
}

fn test_state(name: &str) -> crate::ServerState {
    let root = std::env::temp_dir().join(format!("mediagen-login-routes-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    server_state(root, PluginLoader::empty())
}

fn account_state(state: &crate::ServerState, id: &str) -> String {
    state
        .store
        .account_by_id(id)
        .expect("账号应继续存在")
        .login_state
}

fn active_task(account_id: &str) -> TaskRecord {
    TaskRecord {
        id: "task-active".to_string(),
        platform: "platform-a".to_string(),
        task_kind: "image".to_string(),
        model_id: "example-model".to_string(),
        prompt: "draw".to_string(),
        account_id: Some(account_id.to_string()),
        status: "processing".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: "2026-07-30T00:00:00Z".to_string(),
        completed_at: None,
    }
}
