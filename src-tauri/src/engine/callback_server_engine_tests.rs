use super::control::local_http_client;
use super::*;
use crate::engine::AtomExecutor;
use serde_json::json;

#[tokio::test]
async fn callback_server_returns_raw_request_without_state_validation() {
    let engine = CallbackServerEngine::new();
    let started = start_test_server(&engine).await;
    let port = started.data["port"].as_u64().unwrap();
    assert!(started.data.get("state").is_none());
    assert!(started.data.get("code_challenge").is_none());

    let pending = send_raw_callback(port);
    let request = wait_for_request(&engine, &started.data["server_id"]).await;
    assert_eq!(request.data["path"], "/auth/callback");
    assert_eq!(request.data["query"]["code"], "bad");
    assert_eq!(request.data["query"]["state"], "wrong-state");

    respond_to_request(&engine, &request).await;
    let response = pending.await.unwrap();
    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(response.text().await.unwrap(), "state checked by plugin");

    stop_server(&engine, &started.data["server_id"]).await;
}

#[tokio::test]
async fn callback_server_replaces_owned_server_on_same_port() {
    let engine = CallbackServerEngine::new();
    let first = engine
        .execute(
            "callback_server.start",
            &json!({ "port": 0, "path": "/auth/callback" }),
        )
        .await
        .unwrap();
    let port = first.data["port"].as_u64().unwrap();

    let second = engine
        .execute(
            "callback_server.start",
            &json!({ "port": port, "path": "/auth/callback" }),
        )
        .await
        .unwrap();

    assert_eq!(second.data["port"].as_u64().unwrap(), port);
    let err = engine
        .execute(
            "callback_server.wait_request",
            &json!({
                "server_id": first.data["server_id"],
                "timeout_ms": 10
            }),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("callback server 不存在"));

    let _ = engine
        .execute(
            "callback_server.stop",
            &json!({ "server_id": second.data["server_id"] }),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn callback_server_rejects_reserved_api_port() {
    let engine = CallbackServerEngine::with_reserved_ports(vec![1455]);
    let err = engine
        .execute(
            "callback_server.start",
            &json!({ "port": 1455, "path": "/auth/callback" }),
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("应用 API 端口"));
}

async fn start_test_server(engine: &CallbackServerEngine) -> StepResult {
    engine
        .execute(
            "callback_server.start",
            &json!({
                "port": 0,
                "path": "/auth/callback",
                "redirect_host": "localhost"
            }),
        )
        .await
        .unwrap()
}

fn send_raw_callback(port: u64) -> tokio::task::JoinHandle<reqwest::Response> {
    let client = local_http_client();
    tokio::spawn(async move {
        client
            .get(format!(
                "http://127.0.0.1:{port}/auth/callback?code=bad&state=wrong-state"
            ))
            .send()
            .await
            .unwrap()
    })
}

async fn wait_for_request(
    engine: &CallbackServerEngine,
    server_id: &serde_json::Value,
) -> StepResult {
    engine
        .execute(
            "callback_server.wait_request",
            &json!({
                "server_id": server_id,
                "timeout_ms": 1000
            }),
        )
        .await
        .unwrap()
}

async fn respond_to_request(engine: &CallbackServerEngine, request: &StepResult) {
    let _ = engine
        .execute(
            "callback_server.respond",
            &json!({
                "server_id": request.data["server_id"],
                "request_id": request.data["request_id"],
                "status": 400,
                "body": "state checked by plugin"
            }),
        )
        .await
        .unwrap();
}

async fn stop_server(engine: &CallbackServerEngine, server_id: &serde_json::Value) {
    let _ = engine
        .execute("callback_server.stop", &json!({ "server_id": server_id }))
        .await
        .unwrap();
}
