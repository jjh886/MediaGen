use actix_web::test as actix_test;
use mediagen_browser_gateway::{BrowserSessionCreateRequest, BrowserSessionPurpose};
use serde_json::Value;

use crate::store::{ProxyRecord, TaskRecord};
use crate::test_fixtures::{account_record, server_state};

#[actix_web::test]
async fn task_and_account_routes_return_latest_forty_then_next_page() {
    let app = crate::init_admin_test_app!(seeded_main_list_state());

    let task_response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/api/tasks?platform=fake")
            .to_request(),
    )
    .await;
    assert!(
        task_response.status().is_success(),
        "{}",
        task_response.status()
    );
    let tasks: Value = serde_json::from_slice(&actix_test::read_body(task_response).await).unwrap();
    assert_eq!(tasks["data"].as_array().unwrap().len(), 40);
    assert_eq!(tasks["data"][0]["id"], "task-44");

    let account_response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/api/accounts?platform=fake&limit=40&offset=40")
            .to_request(),
    )
    .await;
    assert!(
        account_response.status().is_success(),
        "{}",
        account_response.status()
    );
    let accounts: Value =
        serde_json::from_slice(&actix_test::read_body(account_response).await).unwrap();
    assert_eq!(accounts["data"].as_array().unwrap().len(), 5);
    assert_eq!(accounts["data"][0]["id"], "acc-04");

    let summary: Value = actix_test::call_and_read_body_json(
        &app,
        actix_test::TestRequest::get()
            .uri("/api/accounts/summary")
            .to_request(),
    )
    .await;
    assert_eq!(summary["data"]["available"], 45);
    assert_eq!(summary["data"]["total"], 45);

    let account: Value = actix_test::call_and_read_body_json(
        &app,
        actix_test::TestRequest::get()
            .uri("/api/accounts/acc-00")
            .to_request(),
    )
    .await;
    assert_eq!(account["data"]["id"], "acc-00");
}

#[actix_web::test]
async fn task_filters_apply_before_pagination_and_options_include_history() {
    let state = server_state(
        test_root("task-filters"),
        mediagen_plugin_runtime::PluginLoader::empty(),
    );
    for index in 0..45 {
        state.store.insert_task(task(index, timestamp(index)));
    }
    for index in 0..3 {
        let mut record = task(index, timestamp(index));
        record.id = format!("filtered-{index}");
        record.task_kind = "video".into();
        record.status = "failed".into();
        record.account_id = Some("deleted-account".into());
        record.model_id = "historic-model".into();
        state.store.insert_task(record);
    }
    let mut other_platform = task(50, timestamp(50));
    other_platform.platform = "other".into();
    other_platform.model_id = "other-model".into();
    state.store.insert_task(other_platform);
    let app = crate::init_admin_test_app!(state);

    for filter in [
        "task_kind=video",
        "status=failed",
        "account_id=deleted-account",
        "model_id=historic-model",
        "task_kind=video&status=failed&account_id=deleted-account&model_id=historic-model",
    ] {
        let body: Value = actix_test::call_and_read_body_json(
            &app,
            actix_test::TestRequest::get()
                .uri(&format!(
                    "/api/tasks?platform=fake&limit=1&offset=1&{filter}"
                ))
                .to_request(),
        )
        .await;
        assert_eq!(body["data"].as_array().unwrap().len(), 1, "{filter}");
        assert_eq!(body["data"][0]["id"], "filtered-1", "{filter}");
    }
    for query in [
        "status=completed&task_kind=video",
        "model_id=missing",
        "task_kind=unknown",
    ] {
        let body: Value = actix_test::call_and_read_body_json(
            &app,
            actix_test::TestRequest::get()
                .uri(&format!("/api/tasks?platform=fake&{query}"))
                .to_request(),
        )
        .await;
        assert!(body["data"].as_array().unwrap().is_empty());
    }
    let options: Value = actix_test::call_and_read_body_json(
        &app,
        actix_test::TestRequest::get()
            .uri("/api/tasks/filter-options?platform=fake")
            .to_request(),
    )
    .await;
    assert_eq!(
        options["data"]["model_ids"],
        serde_json::json!(["fake-model", "historic-model"])
    );
    assert_eq!(
        options["data"]["account_ids"],
        serde_json::json!(["deleted-account"])
    );
}

fn seeded_main_list_state() -> crate::ServerState {
    let state = server_state(
        test_root("main-lists"),
        mediagen_plugin_runtime::PluginLoader::empty(),
    );
    for index in 0..45 {
        let created_at = timestamp(index);
        let mut account = account_record(&format!("acc-{index:02}"), "fake", "账号", "");
        account.created_at = created_at.clone();
        state.store.insert_account(account);
        state.store.insert_task(task(index, created_at));
    }
    state
}

#[actix_web::test]
async fn admin_lists_share_the_same_page_boundary() {
    let state = server_state(
        test_root("admin-lists"),
        mediagen_plugin_runtime::PluginLoader::empty(),
    );
    for index in 0..45 {
        state
            .store
            .create_api_key(format!("key-{index}"), format!("secret-{index}"));
        state.store.create_proxy(ProxyRecord {
            id: format!("proxy-{index:02}"),
            name: format!("代理 {index}"),
            server: "socks5://127.0.0.1:1080".to_string(),
            username: String::new(),
            password: String::new(),
            enabled: true,
            created_at: timestamp(index),
        });
        state.store.add_audit_log(
            "engine",
            "info",
            format!("日志 {index}"),
            String::new(),
            None,
        );
    }
    let app = crate::init_admin_test_app!(state);

    for uri in ["/api/api-keys", "/api/proxies", "/api/logs"] {
        let body: Value = actix_test::call_and_read_body_json(
            &app,
            actix_test::TestRequest::get().uri(uri).to_request(),
        )
        .await;
        assert_eq!(body["data"].as_array().unwrap().len(), 40, "{uri}");
    }
}

#[actix_web::test]
async fn browser_session_list_filters_before_pagination() {
    let state = server_state(
        test_root("browser-sessions"),
        mediagen_plugin_runtime::PluginLoader::empty(),
    );
    for index in 0..3 {
        state
            .browser_gateway
            .create_session(BrowserSessionCreateRequest {
                purpose: BrowserSessionPurpose::HumanVerification,
                task_id: Some(format!("task-{index}")),
                start_url: "https://example.test".to_string(),
                ..BrowserSessionCreateRequest::default()
            })
            .await
            .unwrap();
    }
    state
        .browser_gateway
        .create_session(BrowserSessionCreateRequest {
            purpose: BrowserSessionPurpose::DebugPreview,
            start_url: "https://example.test".to_string(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .unwrap();
    let app = crate::init_admin_test_app!(state);

    let body: Value = actix_test::call_and_read_body_json(
        &app,
        actix_test::TestRequest::get()
            .uri("/api/browser-sessions?takeover_only=true&limit=2&offset=1")
            .to_request(),
    )
    .await;

    assert_eq!(body["data"].as_array().unwrap().len(), 2);
    assert!(body["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|session| session["purpose"] == "human_verification"));
}

fn task(index: usize, created_at: String) -> TaskRecord {
    TaskRecord {
        id: format!("task-{index:02}"),
        platform: "fake".to_string(),
        task_kind: "image".to_string(),
        model_id: "fake-model".to_string(),
        prompt: "test".to_string(),
        account_id: None,
        status: "completed".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at,
        completed_at: None,
    }
}

fn timestamp(index: usize) -> String {
    format!("2026-08-18T00:{index:02}:00Z")
}

fn test_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("mediagen-pagination-route-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    root
}
