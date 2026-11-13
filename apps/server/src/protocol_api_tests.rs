use crate::ServerState;
use actix_web::{test as actix_test, web, App};
use serde_json::{json, Value};

pub(crate) fn state() -> web::Data<ServerState> {
    crate::test_fixtures::protocol_state(false)
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
    let state = crate::test_fixtures::protocol_state(true);
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
    assert_eq!(response.status(), 400);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(body["error"]["code"], "missing_model");
}

#[test]
fn chat_conversion_preserves_images_calls_and_tool_outputs() {
    let value=crate::protocol_request::chat_to_responses(&json!({"model":"text-model","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,YQ=="}}]},{"role":"assistant","content":null,"tool_calls":[{"id":"call_a","type":"function","function":{"name":"weather","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_a","content":"sunny"}]})).unwrap();
    assert_eq!(value["input"][0]["content"][0]["type"], "input_image");
    assert_eq!(value["input"][1]["type"], "function_call");
    assert_eq!(value["input"][2]["call_id"], "call_a");
}

#[actix_web::test]
async fn unavailable_model_or_quota_fails_before_streaming_or_enqueuing() {
    for missing_permission in [true, false] {
        let state = crate::test_fixtures::protocol_state(missing_permission);
        if !missing_permission {
            state
                .store
                .mark_account_state_for_seconds("account", "daily_quota_exhausted", 3600);
            state
                .store
                .record_account_failure("account", "quota", "text");
        }
        let app = actix_test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        for endpoint in ["responses", "chat/completions"] {
            for mode in ["sync", "stream", "async"] {
                let mut body = if endpoint == "responses" {
                    json!({"model":"text-model","input":"test"})
                } else {
                    json!({"model":"text-model","messages":[{"role":"user","content":"test"}]})
                };
                if mode != "sync" {
                    body[mode] = json!(true);
                }
                let response = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    actix_test::call_service(
                        &app,
                        actix_test::TestRequest::post()
                            .uri(&format!("/v1/{endpoint}"))
                            .insert_header(("authorization", "Bearer key-a"))
                            .set_json(body)
                            .to_request(),
                    ),
                )
                .await
                .expect("没有权限或额度时不应等待");
                assert_eq!(
                    response.status().as_u16(),
                    if missing_permission { 403 } else { 429 }
                );
                let body: Value = actix_test::read_body_json(response).await;
                assert_eq!(
                    body["error"]["code"],
                    if missing_permission {
                        "no_available_account_for_model"
                    } else {
                        "insufficient_quota"
                    }
                );
                assert!(body["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("text-model"));
                assert!(state.store.list_tasks(None).is_empty());
                assert_eq!(state.store.active_generation_job_count("text"), 0);
            }
        }
    }
}

#[actix_web::test]
async fn terminal_errors_replay_without_reexecuting_the_plugin() {
    for stream in [true, false] {
        for failure in [
            "conversation_unavailable: original missing",
            "recovery_exhausted: timed out",
            "generation_failed: upstream failed",
            "ProxyConnect: unexpected end of file",
            "model_quota_exhausted; resets_at=2099-11-10T16:51:45Z",
        ] {
            let state = state();
            let script = state
                .plugin_loader
                .get_plugin_dir("test")
                .unwrap()
                .join("scripts/text.js");
            std::fs::write(
                &script,
                format!(
                    "function main(){{host.checkpoint({{phase:'accepted'}});throw new Error({});}}",
                    serde_json::to_string(failure).unwrap()
                ),
            )
            .unwrap();
            let app = actix_test::init_service(
                App::new()
                    .app_data(state.clone())
                    .configure(crate::configure_routes),
            )
            .await;
            let body = json!({"model":"text-model","input":"test","stream":stream});
            for repeat in [false, true] {
                if repeat {
                    std::fs::write(
                        &script,
                        "function main(){throw new Error('unexpected second execution')}",
                    )
                    .unwrap();
                }
                let response = actix_test::call_service(
                    &app,
                    actix_test::TestRequest::post()
                        .uri("/v1/responses")
                        .insert_header(("authorization", "Bearer key-a"))
                        .insert_header(("x-client-request-id", "terminal-replay"))
                        .set_json(&body)
                        .to_request(),
                )
                .await;
                if repeat || !stream {
                    assert_eq!(
                        response.status().as_u16(),
                        if failure.starts_with("model_quota") {
                            429
                        } else {
                            409
                        }
                    );
                }
                let raw = actix_test::read_body(response).await;
                let raw = String::from_utf8_lossy(&raw);
                assert!(!raw.contains("unexpected second execution"));
                assert!(raw.contains(crate::protocol_error::public_failure(failure).1));
            }
            assert_eq!(
                state
                    .store
                    .list_audit_logs()
                    .iter()
                    .filter(|log| log.summary == "生成请求失败")
                    .count(),
                1
            );
        }
    }
}

#[path = "protocol_model_tests.rs"]
mod model_tests;
