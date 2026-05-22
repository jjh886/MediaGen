use super::*;

#[test]
fn test_parse_request_params() {
    let params = json!({
        "method": "POST",
        "url": "https://api.example.com/generate",
        "headers": {
            "x-api-key": "test-key",
            "content-type": "application/json"
        },
        "body": {"prompt": "hello"},
        "cookies": "session=abc123",
        "timeout_ms": 5000
    });

    assert_eq!(params["method"].as_str().unwrap(), "POST");
    assert_eq!(
        params["url"].as_str().unwrap(),
        "https://api.example.com/generate"
    );
    assert_eq!(params["timeout_ms"].as_u64().unwrap(), 5000);

    let headers = build_headers(&params["headers"]);
    assert_eq!(headers.get("x-api-key").unwrap(), "test-key");
    assert_eq!(headers.get("content-type").unwrap(), "application/json");

    assert_eq!(params["cookies"].as_str().unwrap(), "session=abc123");
    assert!(params["body"].is_object());
}

#[test]
fn test_evaluate_condition() {
    let data = json!({
        "data": {
            "status": "completed",
            "progress": 100
        }
    });

    assert!(evaluate_condition(&data, "data.status == 'completed'"));
    assert!(!evaluate_condition(&data, "data.status == 'pending'"));
    assert!(evaluate_condition(&data, "data.progress == '100'"));
}

#[test]
fn test_evaluate_condition_not_equal() {
    let data = json!({
        "data": {
            "status": "processing"
        }
    });

    assert!(evaluate_condition(&data, "data.status != 'completed'"));
    assert!(!evaluate_condition(&data, "data.status != 'processing'"));
}

#[test]
fn http_error_message_includes_response_body() {
    assert_eq!(
        http_error_message(403, "{\"error\":\"unsupported\"}"),
        "HTTP 403: {\"error\":\"unsupported\"}"
    );
}

#[test]
fn request_body_mode_detects_form_payloads() {
    let params = json!({
        "body_format": "form",
        "body": {
            "grant_type": "authorization_code",
            "client_id": "client"
        }
    });

    assert_eq!(body_format(&params), RequestBodyFormat::Form);
}

#[test]
fn request_body_can_be_resolved_from_context_path() {
    let params = json!({
        "context": {
            "payload": { "prompt": "hello" }
        },
        "body_path": "payload"
    });

    assert_eq!(
        request_body_value(&params).unwrap(),
        Some(&json!({ "prompt": "hello" }))
    );
}

#[test]
fn sse_collects_json_data_lines_and_ignores_done_markers() {
    let raw = "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"result\":\"abc\"}}\n\ndata: [DONE]\n";
    let events = sse::collect_sse_events(raw);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["item"]["result"], "abc");
}

#[test]
fn sse_collects_crlf_separated_events() {
    let raw = "event: response.completed\r\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\r\n\r\ndata: [DONE]\r\n";
    let events = sse::collect_sse_events(raw);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "response.completed");
}
