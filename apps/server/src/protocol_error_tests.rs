use super::public_failure;

#[test]
fn input_validation_is_not_misreported_as_model_capability_failure() {
    let raw = "SAFE_RETRY:plugin.run_js JS 错误: Error: invalid_input: 附件文件名无效\n    at apiTextResponsesMedia (eval_script:2240:17)\nprivate-detail";
    let (status, code, public) = public_failure(raw);
    assert_eq!((status, code), (400, "invalid_input"));
    assert_eq!(public, "附件文件名无效，请检查文件名");
    assert!(!public.contains("eval_script"));
    assert!(!public.contains("private-detail"));
    assert_eq!(
        public_failure("invalid_input: private-detail").1,
        "invalid_input"
    );
    assert!(!public_failure("invalid_input: private-detail")
        .2
        .contains("private-detail"));
    assert_eq!(
        public_failure("missing_input: private-detail").1,
        "missing_input"
    );
    assert_eq!(
        public_failure("unsupported_speed: private-detail").1,
        "unsupported_parameter"
    );
}

#[test]
fn model_quota_failure_reports_only_validated_reset_time() {
    let (status, code, public) = public_failure(
        "account_model_unavailable: model_quota_exhausted; model=test; resets_at=2099-11-10T16:51:45.000Z; private-detail",
    );
    assert_eq!((status, code), (429, "insufficient_quota"));
    assert!(public.contains("2099-11-10T16:51:45+00:00"));
    assert!(!public.contains("private-detail"));
    let (_, _, public) = public_failure("model_quota_exhausted; resets_at=private-detail");
    assert!(!public.contains("private-detail"));
    assert!(public.contains("请稍后重试"));
    assert!(!public.contains("其他可用账号"));
}

#[test]
fn explicit_content_refusal_stays_terminal_without_exposing_upstream_details() {
    for message in [
        "content_policy_refusal: private detail",
        "Content Policy violation",
        "safety_violations",
    ] {
        let (status, code, public) = public_failure(message);
        assert_eq!(status, 400);
        assert_eq!(code, "content_policy_violation");
        assert_eq!(public, "上游内容安全规则拒绝了此请求");
    }
    assert_eq!(public_failure("temporary_unavailable").0, 503);
    assert_eq!(public_failure("connection reset").0, 502);
}

#[test]
fn diagnostics_keep_causes_and_remove_credentials_and_request_contents() {
    let text = "HTTP 403 upstream_request_id=up-123 caused by: tls handshake eof\nAuthorization: Bearer private-bearer\nCookie: session=private-cookie\nhttps://user:private-pass@example.test/path?token=private-query#private-fragment\n{\"error\":{\"code\":\"denied\",\"message\":\"detail\"},\"access_token\":\"private-token\",\"messages\":[{\"content\":\"private-prompt\"}]}\npassword=private-password sk-private-key mg_private_key";
    let result = super::sanitize_error(text);
    assert!(result.contains("tls handshake eof"));
    assert!(result.contains("up-123"));
    assert!(result.contains("denied"));
    assert!(result.contains("https://example.test/path"));
    assert!(!result.contains("private-"));
    assert!(!result.contains("mg_private_key"));
    let long = format!("{} final-cause", "原因".repeat(1000));
    assert_eq!(super::sanitize_error(&long), long);
    assert!(super::sanitize_error(&"x".repeat(65537)).contains("已截断"));
    let cause = std::io::Error::other(std::io::Error::other("socket closed"));
    assert!(super::error_chain(&cause).contains("socket closed"));
}

#[actix_web::test]
async fn request_body_accepts_above_old_limit_and_enforces_shared_boundary() {
    use actix_web::{test, App};
    let state = crate::protocol_api_tests::state();
    let app = test::init_service(
        App::new()
            .app_data(state)
            .configure(crate::configure_routes),
    )
    .await;
    for size in [
        16 * 1024 * 1024 + 1,
        crate::MAX_REQUEST_BODY_BYTES,
        crate::MAX_REQUEST_BODY_BYTES + 1,
    ] {
        let mut bytes = br#"{"model":"text-model","input":"hello","store":false}"#.to_vec();
        bytes.resize(size, b' ');
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .insert_header(("content-type", "application/json"))
                .set_payload(bytes)
                .to_request(),
        )
        .await;
        assert_eq!(
            response.status().as_u16(),
            if size <= crate::MAX_REQUEST_BODY_BYTES {
                200
            } else {
                413
            }
        );
        let body: serde_json::Value = test::read_body_json(response).await;
        if size > crate::MAX_REQUEST_BODY_BYTES {
            assert_eq!(body["error"]["code"], "body_too_large");
            assert!(body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("50 MiB"));
        }
    }
}

#[actix_web::test]
async fn both_stream_and_json_failures_record_detailed_private_audit_with_trace_id() {
    use actix_web::{middleware::from_fn, test, App};
    use serde_json::{json, Value};
    let state = crate::protocol_api_tests::state();
    let script = state
        .plugin_loader
        .get_plugin_dir("test")
        .unwrap()
        .join("scripts/text.js");
    std::fs::write(script, r#"function main(ctx) {
        host.emit({type:'response.created',response:{id:'resp_test',object:'response',status:'in_progress',output:[]}});
        throw new Error('HTTP 403 upstream_request_id=up-test ' + 'details '.repeat(100) + 'root-cause-marker access_token=hidden-token key-a');
    }"#).unwrap();
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .wrap(from_fn(crate::request_identity::middleware))
            .configure(crate::configure_routes),
    )
    .await;
    for stream in [false, true] {
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .insert_header(("x-client-request-id", "original-client"))
                .set_json(
                    json!({"model":"text-model","input":"private user prompt","stream":stream}),
                )
                .to_request(),
        )
        .await;
        assert_eq!(response.status().as_u16(), if stream { 200 } else { 502 });
        let trace = response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let bytes = test::read_body(response).await;
        let public = String::from_utf8_lossy(&bytes);
        assert!(!public.contains("root-cause-marker"));
        assert!(!public.contains("hidden-token"));
        let logs = state.store.list_audit_logs();
        let log = logs
            .iter()
            .find(|log| log.summary == "生成请求失败")
            .unwrap();
        let detail: Value = serde_json::from_str(&log.detail).unwrap();
        assert_eq!(detail["request_id"], trace);
        assert_eq!(detail["task_id"], log.task_id.as_ref().unwrap().as_str());
        assert_eq!(detail["platform"], "test");
        assert_eq!(detail["model"], "text-model");
        assert_eq!(detail["account_id"], "account");
        assert!(detail["error"]
            .as_str()
            .unwrap()
            .contains("root-cause-marker"));
        assert!(detail["error"].as_str().unwrap().contains("up-test"));
        for private in ["hidden-token", "key-a", "private user prompt"] {
            assert!(!log.detail.contains(private));
        }
    }
}
