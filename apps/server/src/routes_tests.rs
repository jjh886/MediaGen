use actix_web::{
    body::MessageBody,
    dev::{Service, ServiceResponse},
    test as actix_test,
};
use mediagen_browser_gateway::{
    BrowserSessionCreateRequest, BrowserSessionPurpose, BrowserSessionStatus,
};
use mediagen_plugin_runtime::PluginLoader;
use serde_json::json;

use crate::access_control::hash_account_keys;
use crate::routes::api_key_allowed;
use crate::store::{AccountRecord, TaskRecord};
use crate::test_fixtures::{account_record, server_state};
use crate::ServerState;

macro_rules! post_json {
    ($app:expr, $uri:expr, $body:expr) => {
        call_response(
            &$app,
            actix_test::TestRequest::post()
                .uri($uri)
                .set_json($body)
                .to_request(),
        )
        .await
    };
}

macro_rules! get_json {
    ($app:expr, $uri:expr) => {
        call_json(&$app, actix_test::TestRequest::get().uri($uri).to_request()).await
    };
}

macro_rules! delete_request {
    ($app:expr, $uri:expr) => {
        actix_test::call_service(
            &$app,
            actix_test::TestRequest::delete().uri($uri).to_request(),
        )
        .await
    };
}

#[test]
fn empty_api_key_settings_rejects_openai_requests() {
    let state = test_state("empty-keys");
    let request = actix_test::TestRequest::default().to_http_request();

    assert!(!api_key_allowed(&state, &request));
}

#[test]
fn configured_api_key_allows_matching_bearer_only() {
    let state = test_state("configured-keys");
    state.store.update_settings(crate::store::SettingsUpdate {
        api_keys: Some("key-a".to_string()),
        ..crate::store::SettingsUpdate::default()
    });

    let good = actix_test::TestRequest::default()
        .insert_header(("authorization", "Bearer key-a"))
        .to_http_request();
    let bad = actix_test::TestRequest::default()
        .insert_header(("authorization", "Bearer bad-key"))
        .to_http_request();

    assert!(api_key_allowed(&state, &good));
    assert!(!api_key_allowed(&state, &bad));
}

#[test]
fn account_api_key_allows_bearer_authentication() {
    let state = test_state("account-key-auth");
    let mut account = account_record("acc-key", "fake", "Key Account", "");
    account.api_key = hash_account_keys("account-only-key");
    state.store.insert_account(account);

    let good = actix_test::TestRequest::default()
        .insert_header(("authorization", "Bearer account-only-key"))
        .to_http_request();
    let bad = actix_test::TestRequest::default()
        .insert_header(("authorization", "Bearer bad-key"))
        .to_http_request();

    assert!(api_key_allowed(&state, &good));
    assert!(!api_key_allowed(&state, &bad));
}

#[actix_web::test]
async fn health_exposes_install_root_for_local_launcher() {
    let app = crate::init_admin_test_app!(test_state("health-install-root"));

    let body = get_json!(app, "/health");

    assert_eq!(body["service"], "mediagen-server");
    assert!(body["install_root"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(body["pid"].as_u64().is_some());
}

#[actix_web::test]
async fn cancelling_task_closes_its_active_browser_session() {
    let state = test_state("cancel-task-browser");
    state.store.insert_task(TaskRecord {
        id: "task-browser".to_string(),
        platform: "fake".to_string(),
        task_kind: "image".to_string(),
        model_id: "fake-image".to_string(),
        prompt: "draw".to_string(),
        account_id: Some("account-1".to_string()),
        status: "processing".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: "2026-07-30T00:00:00Z".to_string(),
        completed_at: None,
    });
    let session = state
        .browser_gateway
        .create_session(BrowserSessionCreateRequest {
            purpose: BrowserSessionPurpose::TaskTakeover,
            platform: "fake".to_string(),
            task_id: Some("task-browser".to_string()),
            start_url: "https://example.test".to_string(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .unwrap()
        .session;
    let app = crate::init_admin_test_app!(state.clone());

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/api/tasks/task-browser/cancel")
            .to_request(),
    )
    .await;

    assert!(response.status().is_success());
    assert_eq!(
        state
            .browser_gateway
            .get_session(&session.id)
            .unwrap()
            .status,
        BrowserSessionStatus::Cancelled
    );
}

#[actix_web::test]
async fn admin_api_key_routes_create_list_and_delete() {
    let state = test_state("admin-keys");
    let app = crate::init_admin_test_app!(state.clone());

    let key_response = post_json!(app, "/api/api-keys", json!({ "name": "Web 用户" }));
    assert!(key_response.status().is_success());
    let key_body = read_json(key_response).await;
    let key_id = key_body["data"]["id"].as_str().unwrap();
    let generated_key = key_body["data"]["key"].as_str().unwrap();
    assert!(generated_key.starts_with("mg_"));
    assert!(state.store.api_key_allowed(generated_key));

    let keys = get_json!(app, "/api/api-keys");
    assert_eq!(
        keys["data"][0]["masked_key"],
        key_body["data"]["masked_key"]
    );
    assert!(delete_request!(app, &format!("/api/api-keys/{key_id}"))
        .status()
        .is_success());
    assert_eq!(
        delete_request!(app, &format!("/api/api-keys/{key_id}")).status(),
        actix_web::http::StatusCode::NOT_FOUND
    );
}

#[actix_web::test]
async fn admin_proxy_routes_create_list_and_delete() {
    let app = crate::init_admin_test_app!(test_state("admin-proxies"));

    let bad_proxy = post_json!(app, "/api/proxies", json!({ "server": "http://proxy" }));
    assert_eq!(bad_proxy.status(), actix_web::http::StatusCode::BAD_REQUEST);

    let proxy_response = post_json!(
        app,
        "/api/proxies",
        json!({
            "name": " ",
            "server": "socks5h://proxy.example:1080",
            "username": "user",
            "password": "pass"
        })
    );
    assert!(proxy_response.status().is_success());
    let proxy_body = read_json(proxy_response).await;
    let proxy_id = proxy_body["data"]["id"].as_str().unwrap();

    let proxies = get_json!(app, "/api/proxies");
    assert_eq!(proxies["data"][0]["name"], "SOCKS5 代理");
    assert_eq!(proxies["data"][0]["server"], "socks5h://proxy.example:1080");
    assert!(proxies["data"][0].get("password").is_none());
    assert_eq!(proxies["data"][0]["password_configured"], true);

    assert!(delete_request!(app, &format!("/api/proxies/{proxy_id}"))
        .status()
        .is_success());
    assert_eq!(
        delete_request!(app, "/api/proxies/proxy-missing").status(),
        actix_web::http::StatusCode::NOT_FOUND
    );
}

#[actix_web::test]
async fn admin_proxy_test_route_validates_request() {
    let app = crate::init_admin_test_app!(test_state("admin-proxy-test"));

    let missing_test = post_json!(app, "/api/proxies/proxy-missing/test", json!({}));
    assert_eq!(
        missing_test.status(),
        actix_web::http::StatusCode::NOT_FOUND
    );

    let active_proxy = post_json!(
        app,
        "/api/proxies",
        json!({ "name": "测试代理", "server": "socks5h://proxy.example:1080" })
    );
    let active_body = read_json(active_proxy).await;
    let active_id = active_body["data"]["id"].as_str().unwrap();
    let invalid_target_test = post_json!(
        app,
        &format!("/api/proxies/{active_id}/test"),
        json!({ "url": "ftp://example.com" })
    );
    assert_eq!(
        invalid_target_test.status(),
        actix_web::http::StatusCode::BAD_REQUEST
    );

    let disabled_proxy = post_json!(
        app,
        "/api/proxies",
        json!({
            "name": "停用代理",
            "server": "socks5://disabled.example:1080",
            "enabled": false
        })
    );
    let disabled_body = read_json(disabled_proxy).await;
    let disabled_id = disabled_body["data"]["id"].as_str().unwrap();
    let disabled_test = post_json!(app, &format!("/api/proxies/{disabled_id}/test"), json!({}));
    assert_eq!(
        disabled_test.status(),
        actix_web::http::StatusCode::BAD_REQUEST
    );
}

#[actix_web::test]
async fn admin_logs_and_batch_registration_routes_work() {
    let root = test_root("admin-logs-batch");
    let _ = std::fs::remove_dir_all(&root);
    let plugins_dir = root.join("plugins");
    write_stream_plugin(&plugins_dir);
    let state = test_state_with_plugins("admin-logs-batch", &plugins_dir);
    state.store.add_audit_log(
        "engine",
        "error",
        "任务失败".to_string(),
        "task scoped log".to_string(),
        Some("task-log-1".to_string()),
    );
    let app = crate::init_admin_test_app!(state);

    let batch = post_json!(
        app,
        "/api/accounts/batch-register",
        json!({ "platform": "fake", "count": 2, "password": "pwd", "birthday": "1990-01-01" })
    );
    assert_eq!(batch.status(), actix_web::http::StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        post_json!(app, "/api/cache/clear", json!({})).status(),
        actix_web::http::StatusCode::NOT_IMPLEMENTED
    );

    let logs = get_json!(app, "/api/logs");
    assert!(!logs["data"].as_array().unwrap().is_empty());
    let filtered_logs = get_json!(app, "/api/logs?module=engine&level=warn");
    assert_eq!(filtered_logs["data"].as_array().unwrap().len(), 1);
    let empty_logs = get_json!(app, "/api/logs?module=api");
    assert!(empty_logs["data"].as_array().unwrap().is_empty());
    let task_logs = get_json!(app, "/api/logs?task_id=task-log-1");
    assert_eq!(task_logs["data"].as_array().unwrap().len(), 1);
    assert_eq!(task_logs["data"][0]["task_id"], "task-log-1");
    assert!(post_json!(app, "/api/logs/clear", json!({}))
        .status()
        .is_success());
    assert!(get_json!(app, "/api/logs/audit")["data"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[actix_web::test]
async fn stream_image_generation_returns_sse_response() {
    let root = test_root("stream-images");
    let _ = std::fs::remove_dir_all(&root);
    let plugins_dir = root.join("plugins");
    write_stream_plugin(&plugins_dir);
    let state = test_state_with_plugins("stream-images", &plugins_dir);
    state.store.update_settings(crate::store::SettingsUpdate {
        api_keys: Some("stream-key".to_string()),
        ..crate::store::SettingsUpdate::default()
    });
    state.store.insert_account(account("fake"));

    let app = crate::init_admin_test_app!(state);
    let request = actix_test::TestRequest::post()
        .uri("/v1/images/generations")
        .insert_header(("authorization", "Bearer stream-key"))
        .set_json(json!({
            "model": "gpt-image-test",
            "prompt": "draw a square",
            "stream": true
        }))
        .to_request();

    let response = actix_test::call_service(&app, request).await;
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = actix_test::read_body(response).await;
    let text = String::from_utf8(body.to_vec()).unwrap();

    assert!(status.is_success());
    assert!(content_type.starts_with("text/event-stream"));
    assert!(text.contains("event: image_generation.completed"));
    assert!(!text.contains("data: [DONE]"));
    assert!(text.contains("\"type\":\"image_generation.completed\""));

    let edit_request = actix_test::TestRequest::post()
        .uri("/v1/images/edits")
        .insert_header(("authorization", "Bearer stream-key"))
        .set_json(json!({
            "model": "gpt-image-test",
            "prompt": "edit a square",
            "stream": true,
            "images": [{ "image_url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGNkYPjPwMDAxMDAwMDAAAALHwEDmIWXfgAAAABJRU5ErkJggg==" }]
        }))
        .to_request();
    let edit_response = actix_test::call_service(&app, edit_request).await;
    let edit_body = actix_test::read_body(edit_response).await;
    let edit_text = String::from_utf8(edit_body.to_vec()).unwrap();

    assert!(edit_text.contains("event: image_edit.completed"));
    assert!(!edit_text.contains("event: image_generation.completed"));
}

async fn call_json<S, R, B, E>(app: &S, request: R) -> serde_json::Value
where
    S: Service<R, Response = ServiceResponse<B>, Error = E>,
    B: MessageBody + 'static,
    E: std::fmt::Debug,
{
    let response = actix_test::call_service(app, request).await;
    assert!(response.status().is_success());
    read_json(response).await
}

async fn call_response<S, R, B, E>(app: &S, request: R) -> ServiceResponse<B>
where
    S: Service<R, Response = ServiceResponse<B>, Error = E>,
    B: MessageBody + 'static,
    E: std::fmt::Debug,
{
    actix_test::call_service(app, request).await
}

async fn read_json<B>(response: ServiceResponse<B>) -> serde_json::Value
where
    B: MessageBody + 'static,
{
    let body = actix_test::read_body(response).await;
    serde_json::from_slice(&body).unwrap()
}

fn test_state(name: &str) -> ServerState {
    let root = test_root(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    state_for_root(root, PluginLoader::empty())
}

fn test_state_with_plugins(name: &str, plugins_dir: &std::path::Path) -> ServerState {
    let root = test_root(name);
    std::fs::create_dir_all(&root).unwrap();
    state_for_root(root, PluginLoader::load_from(plugins_dir).unwrap())
}

fn state_for_root(root: std::path::PathBuf, plugin_loader: PluginLoader) -> ServerState {
    server_state(root, plugin_loader)
}

fn test_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mediagen-routes-test-{name}"))
}

fn write_stream_plugin(root: &std::path::Path) {
    let plugin_dir = root.join("fake");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(root.join("plugins.index.json"), r#"{"plugins":["fake"]}"#).unwrap();
    std::fs::write(
        plugin_dir.join("manifest.json"),
        r#"{
          "platform_id": "fake",
          "display_name": "Fake",
          "adapter_mode": "api",
          "models": [
            { "model_id": "fake-model", "display_name": "Fake Model", "aliases": ["gpt-image-test"] }
          ],
          "flows": {
            "generate_image": {
              "steps": [
                {
                  "action": "plugin.run_js",
                  "script": "function main() { return { images: [{ url: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGNkYPjPwMDAxMDAwMDAAAALHwEDmIWXfgAAAABJRU5ErkJggg==' }] }; }",
                  "save_as": "result"
                }
              ]
            }
          }
        }"#,
    )
    .unwrap();
}

fn account(platform: &str) -> AccountRecord {
    account_record("acc-stream", platform, "stream account", "")
}
