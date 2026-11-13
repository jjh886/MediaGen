use super::*;
use actix_web::{test, App};
use serde_json::json;

#[actix_web::test]
async fn sync_is_default_and_async_requires_boolean_without_streaming() {
    let state = crate::protocol_api_tests::state();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    for flag in [
        None,
        Some(json!(false)),
        Some(json!("true")),
        Some(json!(null)),
    ] {
        let mut body = json!({"model":"text-model","input":"同步"});
        if let Some(flag) = flag {
            body["async"] = flag;
        }
        let expected = if body.get("async").is_some_and(|flag| !flag.is_boolean()) {
            400
        } else {
            200
        };
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .set_json(body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status().as_u16(), expected);
        if expected == 200 {
            let body: Value = test::read_body_json(response).await;
            assert_eq!(body["object"], "response");
            assert!(body.get("taskId").is_none());
        }
        assert_eq!(state.store.active_generation_job_count("text"), 0);
    }
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/v1/responses")
            .insert_header(("authorization", "Bearer key-a"))
            .set_json(json!({"model":"text-model","input":"test","async":true,"stream":true}))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(state.store.active_generation_job_count("text"), 0);
}

#[actix_web::test]
async fn text_callback_requires_async_and_valid_http_url() {
    for endpoint in ["responses", "chat"] {
        let base = if endpoint == "chat" {
            json!({"model":"text-model","messages":[{"role":"user","content":"test"}]})
        } else {
            json!({"model":"text-model","input":"test"})
        };
        for url in [
            json!(null),
            json!(42),
            json!(""),
            json!(" "),
            json!("file:///tmp/result"),
            json!("https://user:pass@example.test/callback"),
            json!("x".repeat(2049)),
        ] {
            let mut body = base.clone();
            body["async"] = json!(true);
            body["callback_url"] = url;
            assert!(crate::protocol_request::validate_request(&body, endpoint).is_err());
        }
        for flag in [None, Some(false), Some(true)] {
            let mut body = base.clone();
            body["callback_url"] = json!(" https://example.test/callback?token=secret ");
            if let Some(flag) = flag {
                body["async"] = json!(flag);
            }
            assert_eq!(
                crate::protocol_request::validate_request(&body, endpoint).is_ok(),
                flag == Some(true)
            );
        }
    }
}
