use actix_web::{test as actix_test, web, App};
use mediagen_plugin_runtime::PluginLoader;
use serde_json::json;

use crate::access_control::hash_account_keys;
use crate::store::SettingsUpdate;
use crate::test_fixtures::{account_record, server_state};
use crate::ServerState;

#[actix_web::test]
async fn async_generation_returns_task_id_and_persists_encrypted_queue_item() {
    let state = queue_state("image-async-api");
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/images/generations")
            .insert_header(("authorization", "Bearer image-key"))
            .set_json(json!({ "model": "fake-image", "prompt": "draw", "async": true }))
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), actix_web::http::StatusCode::ACCEPTED);
    let payload: serde_json::Value = actix_test::read_body_json(response).await;
    assert_eq!(payload["data"]["status"], "queued");
    let task_id = payload["data"]["taskId"].as_str().unwrap();
    assert!(task_id.starts_with("task-"));

    let queue_response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/api/v2/generation-queue?limit=20&offset=0")
            .insert_header(("authorization", "Bearer image-key"))
            .to_request(),
    )
    .await;
    assert!(queue_response.status().is_success());
    let queue: serde_json::Value = actix_test::read_body_json(queue_response).await;
    assert_eq!(queue["data"]["total"], 1);
    assert_eq!(queue["data"]["items"][0]["taskId"], task_id);

    let jobs = state
        .store
        .generation_jobs
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let job = jobs.iter().find(|job| job.task_id == task_id).unwrap();
    assert!(!job.credential_ciphertext.contains("image-key"));
}

#[actix_web::test]
async fn async_generation_worker_executes_persisted_job() {
    let state = queue_state("image-async-worker");
    let data = web::Data::new(state.clone());
    crate::generation_queue::start_worker(data.clone());
    let app =
        actix_test::init_service(App::new().app_data(data).configure(crate::configure_routes))
            .await;
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/images/generations")
            .insert_header(("authorization", "Bearer image-key"))
            .set_json(json!({ "model": "fake-image", "prompt": "draw", "async": true }))
            .to_request(),
    )
    .await;
    let payload: serde_json::Value = actix_test::read_body_json(response).await;
    let task_id = payload["data"]["taskId"].as_str().unwrap();

    for _ in 0..100 {
        if state
            .store
            .task_by_id(task_id)
            .is_some_and(|task| task.status == "completed")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    assert_eq!(state.store.task_by_id(task_id).unwrap().status, "completed");
    assert_eq!(
        state.store.generation_job(task_id, None).unwrap().state,
        crate::store::GenerationJobState::Completed
    );
}

#[actix_web::test]
async fn async_generation_posts_callback_with_openai_image_data() {
    let (callback_url, received) = callback_receiver().await;
    let state = queue_state("image-async-callback");
    let data = web::Data::new(state.clone());
    crate::generation_queue::start_worker(data.clone());
    let app =
        actix_test::init_service(App::new().app_data(data).configure(crate::configure_routes))
            .await;
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/images/generations")
            .insert_header(("authorization", "Bearer image-key"))
            .set_json(json!({
                "model": "fake-image",
                "prompt": "draw",
                "async": true,
                "callback_url": callback_url
            }))
            .to_request(),
    )
    .await;
    let payload: serde_json::Value = actix_test::read_body_json(response).await;
    let task_id = payload["data"]["taskId"].as_str().unwrap();

    let callback = read_callback_json(received).await;
    assert_eq!(callback["event"], "image_generation.completed");
    assert_eq!(callback["task_id"], task_id);
    assert_eq!(callback["status"], "completed");
    assert_eq!(callback["data"]["data"][0]["b64_json"], "aGVsbG8=");
}

#[actix_web::test]
async fn cancelling_queued_generation_posts_callback() {
    let (callback_url, received) = callback_receiver().await;
    let state = queue_state("image-async-cancel-callback");
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/images/generations")
            .insert_header(("authorization", "Bearer image-key"))
            .set_json(json!({
                "model": "fake-image",
                "prompt": "draw",
                "async": true,
                "callback_url": callback_url
            }))
            .to_request(),
    )
    .await;
    let payload: serde_json::Value = actix_test::read_body_json(response).await;
    let task_id = payload["data"]["taskId"].as_str().unwrap().to_string();

    let action_response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/api/v2/generation-queue/actions")
            .insert_header(("authorization", "Bearer image-key"))
            .set_json(json!({ "action": "cancel", "taskIds": [task_id] }))
            .to_request(),
    )
    .await;
    let action: serde_json::Value = actix_test::read_body_json(action_response).await;
    assert_eq!(action["data"]["affected"], 1);

    let callback = read_callback_json(received).await;
    assert_eq!(callback["event"], "image_generation.cancelled");
    assert_eq!(callback["status"], "cancelled");
}

#[actix_web::test]
async fn image_generation_failover_switches_to_next_account() {
    let state = failover_state();
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let response = actix_test::call_service(&app, image_request().to_request()).await;

    assert!(response.status().is_success());
    assert_primary_cooldown(&state);
    assert_eq!(
        state.store.list_tasks(Some("fake"))[0]
            .account_id
            .as_deref(),
        Some("acc-backup")
    );
    let stored_result = state.store.list_tasks(Some("fake"))[0]
        .result_data
        .clone()
        .unwrap_or_default();
    assert!(!stored_result.contains("data:image/"));
    assert!(!stored_result.contains("base64,"));
}

#[actix_web::test]
async fn account_api_key_can_call_image_generation() {
    let state = account_key_state();
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let response = actix_test::call_service(
        &app,
        image_request_with_key("account-image-key").to_request(),
    )
    .await;

    assert!(response.status().is_success());
    assert_eq!(
        state.store.list_tasks(Some("fake"))[0]
            .account_id
            .as_deref(),
        Some("acc-account-key")
    );
}

#[actix_web::test]
async fn needs_verification_marks_account_state() {
    let state = verification_state();
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let response =
        actix_test::call_service(&app, image_request_with_key("verify-key").to_request()).await;

    assert_eq!(response.status(), actix_web::http::StatusCode::CONFLICT);
    let account = state
        .store
        .list_accounts(Some("fake"))
        .into_iter()
        .find(|account| account.id == "acc-verify")
        .unwrap();
    assert_eq!(account.login_state, "verification_required");
    assert_eq!(
        state.store.list_tasks(Some("fake"))[0].status,
        "verification_required"
    );
}

fn failover_state() -> ServerState {
    let plugins_dir = write_failover_plugin("image-failover");
    let state = server_state(
        test_root("image-failover"),
        PluginLoader::load_from(&plugins_dir).unwrap(),
    );
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("image-key".to_string()),
        ..SettingsUpdate::default()
    });
    state
        .store
        .insert_account(account_record("acc-primary", "fake", "Primary", ""));
    state
        .store
        .insert_account(account_record("acc-backup", "fake", "Backup", ""));
    state
}

fn account_key_state() -> ServerState {
    let plugins_dir = write_failover_plugin("image-account-key");
    let state = server_state(
        test_root("image-account-key"),
        PluginLoader::load_from(&plugins_dir).unwrap(),
    );
    let mut account = account_record("acc-account-key", "fake", "Account Key", "");
    account.api_key = hash_account_keys("account-image-key");
    state.store.insert_account(account);
    state
}

fn verification_state() -> ServerState {
    let plugins_dir = write_verification_plugin("image-verification");
    let state = server_state(
        test_root("image-verification"),
        PluginLoader::load_from(&plugins_dir).unwrap(),
    );
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("verify-key".to_string()),
        ..SettingsUpdate::default()
    });
    state
        .store
        .insert_account(account_record("acc-verify", "fake", "Verify Account", ""));
    state
}

fn queue_state(prefix: &str) -> ServerState {
    let name = format!("{prefix}-{}", uuid::Uuid::new_v4());
    let plugins_dir = write_failover_plugin(&name);
    let state = server_state(
        test_root(&name),
        PluginLoader::load_from(&plugins_dir).unwrap(),
    );
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("image-key".to_string()),
        ..SettingsUpdate::default()
    });
    state
        .store
        .insert_account(account_record("acc-queue", "fake", "Queue", ""));
    state
}

fn image_request() -> actix_web::test::TestRequest {
    image_request_with_key("image-key")
}

fn image_request_with_key(key: &str) -> actix_web::test::TestRequest {
    actix_test::TestRequest::post()
        .uri("/v1/images/generations")
        .insert_header(("authorization", format!("Bearer {key}")))
        .set_json(json!({ "model": "fake-image", "prompt": "draw" }))
}

fn assert_primary_cooldown(state: &ServerState) {
    assert_eq!(
        state
            .store
            .list_accounts(Some("fake"))
            .into_iter()
            .find(|account| account.id == "acc-primary")
            .unwrap()
            .login_state,
        "cooldown"
    );
}

fn write_failover_plugin(name: &str) -> std::path::PathBuf {
    let root = test_root(name).join("plugins");
    let plugin_dir = root.join("fake");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(root.join("plugins.index.json"), r#"{"plugins":["fake"]}"#).unwrap();
    std::fs::write(plugin_dir.join("manifest.json"), failover_manifest()).unwrap();
    root
}

fn write_verification_plugin(name: &str) -> std::path::PathBuf {
    let root = test_root(name).join("plugins");
    let plugin_dir = root.join("fake");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(root.join("plugins.index.json"), r#"{"plugins":["fake"]}"#).unwrap();
    std::fs::write(plugin_dir.join("manifest.json"), verification_manifest()).unwrap();
    root
}

fn failover_manifest() -> &'static str {
    r#"{
      "platform_id": "fake",
      "display_name": "Fake",
      "adapter_mode": "api",
      "models": [{ "model_id": "fake-image", "display_name": "Fake Image" }],
      "failure_strategy": [
        {
          "contains_any": ["rate limit"],
          "category": "account_rate_limited",
          "retry_action": "failover",
          "account_state": "cooldown",
          "usage_consumed": false
        }
      ],
      "flows": {
        "generate_image": {
          "steps": [
            {
              "action": "plugin.run_js",
              "script": "function main(ctx) { if (ctx.account_id === 'acc-primary') return { status: 'error', message: 'rate limit' }; return { images: [{ url: 'data:image/png;base64,aGVsbG8=' }] }; }",
              "save_as": "result"
            }
          ]
        }
      }
    }"#
}

fn verification_manifest() -> &'static str {
    r#"{
      "platform_id": "fake",
      "display_name": "Fake",
      "adapter_mode": "api",
      "workspace_url": "https://example.test/workspace",
      "models": [{ "model_id": "fake-image", "display_name": "Fake Image" }],
      "flows": {
        "generate_image": {
          "steps": [
            {
              "action": "plugin.run_js",
              "script": "function main() { return { status: 'needs_verification', message: 'passkey required' }; }",
              "save_as": "result"
            }
          ]
        }
      }
    }"#
}

fn test_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mediagen-image-api-test-{name}"))
}

async fn callback_receiver() -> (String, tokio::sync::oneshot::Receiver<String>) {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let url = format!("http://{}/callback", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let body = read_callback_body(&mut stream).await;
        let _ = stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .await;
        let _ = tx.send(body);
    });
    (url, rx)
}

async fn read_callback_json(received: tokio::sync::oneshot::Receiver<String>) -> serde_json::Value {
    let body = tokio::time::timeout(std::time::Duration::from_secs(5), received)
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&body).unwrap()
}

async fn read_callback_body(stream: &mut tokio::net::TcpStream) -> String {
    use tokio::io::AsyncReadExt;

    let mut buffer = Vec::new();
    let mut header_len = None;
    let mut content_len = 0usize;
    loop {
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk).await.unwrap();
        if read == 0 {
            return String::new();
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_len.is_none() {
            if let Some(index) = header_end(&buffer) {
                header_len = Some(index + 4);
                content_len = content_length(&buffer[..index]);
            }
        }
        if let Some(start) = header_len {
            if buffer.len() >= start + content_len {
                return String::from_utf8_lossy(&buffer[start..start + content_len]).to_string();
            }
        }
    }
}

fn header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(headers: &[u8]) -> usize {
    String::from_utf8_lossy(headers)
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0)
}
