use actix_web::{body::MessageBody, dev::ServiceResponse, test as actix_test};
use mediagen_browser_gateway::{
    BrowserSessionCreateRequest, BrowserSessionPurpose, BrowserSessionStatus,
};
use mediagen_plugin_runtime::PluginLoader;
use serde_json::json;

use crate::store::{ProxyRecord, TaskRecord};
use crate::test_fixtures::{account_record, server_state};
use crate::ServerState;

macro_rules! post_browser_session {
    ($app:expr, $body:expr) => {
        actix_test::call_service(
            &$app,
            actix_test::TestRequest::post()
                .uri("/api/browser-sessions")
                .set_json($body)
                .to_request(),
        )
        .await
    };
}

macro_rules! complete_browser_session {
    ($app:expr, $id:expr) => {
        actix_test::call_service(
            &$app,
            actix_test::TestRequest::post()
                .uri(&format!("/api/browser-sessions/{}/complete", $id))
                .to_request(),
        )
        .await
    };
}

#[actix_web::test]
async fn saved_proxy_id_is_resolved_before_creating_browser_session() {
    let state = test_state("saved-proxy-session");
    insert_proxy(&state, "proxy-1", true, "user", "pass");
    let app = crate::init_admin_test_app!(state.clone());

    let response = post_browser_session!(
        app,
        json!({
            "purpose": "add_account",
            "platform": "fake",
            "start_url": "https://example.test",
            "proxy_id": "proxy-1",
            "metadata": { "proxy_server": "socks5://old.example:1080" }
        })
    );

    assert!(response.status().is_success());
    let body = read_json(response).await;
    assert_eq!(
        body["session"]["metadata"]["proxy_server"],
        "socks5h://proxy.example:1080"
    );
    assert!(body["session"]["metadata"].get("proxy_password").is_none());
    let internal = session_from_body(&state, &body);
    assert_eq!(internal.metadata["proxy_username"], "user");
    assert_eq!(internal.metadata["proxy_password"], "pass");
}

#[actix_web::test]
async fn relogin_session_uses_saved_proxy_from_account() {
    let state = test_state("relogin-saved-proxy");
    insert_proxy(&state, "proxy-1", true, "user", "pass");
    let mut account = account_record("acc-1", "fake", "Fake", "");
    account.proxy_id = "proxy-1".to_string();
    account.profile_dir = "/profiles/acc-1".to_string();
    account.browser_os = "macos".to_string();
    account.browser_version = "145".to_string();
    state.store.insert_account(account);
    let occupied = state
        .browser_gateway
        .create_session(BrowserSessionCreateRequest {
            purpose: BrowserSessionPurpose::DebugPreview,
            profile_dir: Some("/profiles/acc-1".to_string()),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .unwrap()
        .session;
    let app = crate::init_admin_test_app!(state.clone());

    let response = post_browser_session!(
        app,
        json!({
            "purpose": "relogin_account",
            "platform": "fake",
            "account_id": "acc-1",
            "start_url": "https://example.test"
        })
    );

    assert!(response.status().is_success());
    let body = read_json(response).await;
    assert_eq!(
        body["session"]["metadata"]["proxy_server"],
        "socks5h://proxy.example:1080"
    );
    let session = session_from_body(&state, &body);
    assert_eq!(session.proxy_id.as_deref(), Some("proxy-1"));
    assert_eq!(session.profile_dir, "/profiles/acc-1");
    assert_eq!(
        state
            .browser_gateway
            .get_session(&occupied.id)
            .unwrap()
            .status,
        BrowserSessionStatus::Cancelled
    );
    assert_eq!(session.metadata["proxy_username"], "user");
    assert_eq!(session.metadata["proxy_password"], "pass");
    assert_eq!(session.metadata["browser_os"], "macos");
    assert_eq!(session.metadata["browser_version"], "145");
}

#[actix_web::test]
async fn relogin_session_direct_mode_does_not_reuse_account_proxy() {
    let state = test_state("relogin-direct-proxy");
    insert_proxy(&state, "proxy-1", true, "user", "pass");
    let mut account = account_record("acc-direct", "fake", "Fake", "");
    account.proxy_id = "proxy-1".to_string();
    account.proxy_url = "socks5h://manual.example:1080".to_string();
    state.store.insert_account(account);
    let app = crate::init_admin_test_app!(state.clone());

    let response = post_browser_session!(
        app,
        json!({
            "purpose": "relogin_account",
            "platform": "fake",
            "account_id": "acc-direct",
            "start_url": "https://example.test",
            "metadata": { "proxy_mode": "direct" }
        })
    );

    assert!(response.status().is_success());
    let body = read_json(response).await;
    assert!(body["session"]["metadata"].get("proxy_server").is_none());
    let session = session_from_body(&state, &body);
    assert!(session.proxy_id.is_none());
    assert!(!session.metadata.contains_key("proxy_server"));
    assert_eq!(session.metadata["proxy_mode"], "direct");
}

#[actix_web::test]
async fn relogin_session_keeps_manual_proxy_credentials_internally() {
    let state = test_state("relogin-manual-proxy");
    let mut account = account_record("acc-1", "fake", "Fake", "");
    account.proxy_url = "socks5h://user:pass@proxy.example:1080".to_string();
    state.store.insert_account(account);
    let app = crate::init_admin_test_app!(state.clone());

    let response = post_browser_session!(
        app,
        json!({
            "purpose": "relogin_account",
            "platform": "fake",
            "account_id": "acc-1",
            "start_url": "https://example.test"
        })
    );

    assert!(response.status().is_success());
    let body = read_json(response).await;
    assert_eq!(
        body["session"]["metadata"]["proxy_server"],
        "socks5h://proxy.example:1080"
    );
    let session = session_from_body(&state, &body);
    assert_eq!(
        session.metadata["proxy_server"],
        "socks5h://user:pass@proxy.example:1080"
    );
}

#[actix_web::test]
async fn disabled_or_missing_saved_proxy_rejects_browser_session() {
    let state = test_state("saved-proxy-errors");
    insert_proxy(&state, "proxy-disabled", false, "", "");
    let app = crate::init_admin_test_app!(state);

    let disabled = post_browser_session!(
        app,
        json!({
            "purpose": "add_account",
            "platform": "fake",
            "start_url": "https://example.test",
            "proxy_id": "proxy-disabled"
        })
    );
    let missing = post_browser_session!(
        app,
        json!({
            "purpose": "add_account",
            "platform": "fake",
            "start_url": "https://example.test",
            "proxy_id": "proxy-missing"
        })
    );

    assert_eq!(disabled.status(), actix_web::http::StatusCode::BAD_REQUEST);
    assert_eq!(missing.status(), actix_web::http::StatusCode::BAD_REQUEST);
}

#[actix_web::test]
async fn human_verification_completion_resumes_task_and_account() {
    let state = test_state("human-verification-complete");
    let mut account = account_record("acc-verify", "fake", "Fake", "");
    account.login_state = "verification_required".to_string();
    state.store.insert_account(account);
    state.store.insert_task(TaskRecord {
        id: "task-verify".to_string(),
        platform: "fake".to_string(),
        task_kind: "image".to_string(),
        model_id: "model-1".to_string(),
        prompt: "draw".to_string(),
        account_id: Some("acc-verify".to_string()),
        status: "verification_required".to_string(),
        result_url: None,
        storage_key: None,
        error_message: Some("需要验证".to_string()),
        result_data: None,
        created_at: "2026-06-29T00:00:00Z".to_string(),
        completed_at: None,
    });
    let app = crate::init_admin_test_app!(state.clone());

    let create_response = post_browser_session!(
        app,
        json!({
            "purpose": "human_verification",
            "platform": "fake",
            "account_id": "acc-verify",
            "task_id": "task-verify",
            "start_url": "https://example.test"
        })
    );
    let body = read_json(create_response).await;
    let session_id = body["session"]["id"].as_str().unwrap();
    let complete_response = complete_browser_session!(app, session_id);

    assert!(complete_response.status().is_success());
    assert_eq!(
        state.store.account_by_id("acc-verify").unwrap().login_state,
        "available"
    );
    let task = state
        .store
        .list_tasks(Some("fake"))
        .into_iter()
        .find(|task| task.id == "task-verify")
        .unwrap();
    assert_eq!(task.status, "queued");
    assert!(task.error_message.is_none());
}

async fn read_json<B>(response: ServiceResponse<B>) -> serde_json::Value
where
    B: MessageBody + 'static,
{
    let body = actix_test::read_body(response).await;
    serde_json::from_slice(&body).unwrap()
}

fn session_from_body(
    state: &ServerState,
    body: &serde_json::Value,
) -> mediagen_browser_gateway::BrowserSession {
    state
        .browser_gateway
        .get_session(body["session"]["id"].as_str().unwrap())
        .unwrap()
}

fn insert_proxy(state: &ServerState, id: &str, enabled: bool, username: &str, password: &str) {
    state.store.create_proxy(ProxyRecord {
        id: id.to_string(),
        name: "住宅代理".to_string(),
        server: "socks5h://proxy.example:1080".to_string(),
        username: username.to_string(),
        password: password.to_string(),
        enabled,
        created_at: "2026-06-29T00:00:00Z".to_string(),
    });
}

fn test_state(name: &str) -> ServerState {
    let root = std::env::temp_dir().join(format!("mediagen-browser-session-routes-test-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    server_state(root, PluginLoader::empty())
}
