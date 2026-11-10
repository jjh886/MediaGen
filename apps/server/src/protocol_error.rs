use actix_web::HttpResponse;
use serde_json::json;

pub(crate) fn error(status: u16, code: &str, message: &str) -> HttpResponse {
    let kind = error_kind(status);
    HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status)
            .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
    )
    .json(json!({"error":{"message":message,"type":kind,"param":null,"code":code}}))
}

pub(crate) fn error_kind(status: u16) -> &'static str {
    match status {
        401 => "authentication_error",
        403 => "permission_error",
        429 => "rate_limit_error",
        500..=599 => "server_error",
        _ => "invalid_request_error",
    }
}

pub(crate) fn public_failure(message: &str) -> (u16, &'static str, String) {
    if message == "task_cancelled" {
        return (409, "task_cancelled", "请求已取消".into());
    }
    if message.contains("model_quota_exhausted") {
        let reset = message
            .split(';')
            .find_map(|part| part.trim().strip_prefix("resets_at="))
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok());
        let detail = reset.map_or_else(
            || "当前请求的模型额度暂不可用，请稍后重试".into(),
            |time| {
                format!(
                    "当前请求的模型额度暂不可用，上游返回的恢复时间：{}，请稍后重试",
                    time.to_rfc3339()
                )
            },
        );
        return (429, "rate_limit_exceeded", detail);
    }
    if message.contains("account_model_unavailable") {
        return (
            409,
            "account_model_unavailable",
            "当前账号不支持所选模型，请刷新账号模型目录；已有会话需恢复原账号能力".into(),
        );
    }
    let lower = message.to_lowercase();
    if [
        "content_policy",
        "content policy",
        "safety policy",
        "safety_violations",
        "safety system",
    ]
    .iter()
    .any(|code| lower.contains(code))
    {
        return (
            400,
            "content_policy_violation",
            "上游内容安全规则拒绝了此请求".into(),
        );
    }
    if message.contains("invalid_input")
        || message.contains("missing_input")
        || message.contains("unsupported_")
    {
        (
            400,
            "unsupported_parameter",
            "当前模型不支持请求中的参数，请查询模型能力".into(),
        )
    } else if message.contains("account_permission_denied") {
        (
            403,
            "account_permission_denied",
            "当前上游账号未获此能力授权，请检查订阅或账号权限".into(),
        )
    } else if message.contains("insufficient_quota") {
        (
            429,
            "insufficient_quota",
            "当前模型的上游账号额度为 0，请检查 API 额度或订阅".into(),
        )
    } else if message.contains("429") || message.contains("quota_exhausted") {
        (
            429,
            "rate_limit_exceeded",
            "上游额度或速率限制，请稍后重试".into(),
        )
    } else if message.contains("verification_required") || message.contains("needs_verification") {
        (
            409,
            "verification_required",
            "上游账号需要人工验证，请从控制台账号页面打开验证窗口".into(),
        )
    } else if message.contains("upstream_client_outdated") {
        (
            503,
            "upstream_client_outdated",
            "上游要求升级客户端协议，请更新对应插件后重试".into(),
        )
    } else if message.contains("temporary_unavailable") {
        (
            503,
            "temporary_unavailable",
            "原任务暂未完成，请使用相同客户端请求编号继续恢复".into(),
        )
    } else if message.contains("LOGIN_EXPIRED") {
        (
            503,
            "account_authentication_failed",
            "上游账号需要重新登录".into(),
        )
    } else {
        (502, "upstream_error", "上游请求未成功完成".into())
    }
}

pub(crate) fn record_failure(
    state: &crate::ServerState,
    context: &crate::media_execution::MediaFlowContext<'_>,
    request_id: &str,
    endpoint: &str,
    decision: &crate::failure_strategy::FailureDecision,
    caller_key: &str,
) {
    let (status, code, _) = public_failure(&decision.original_message);
    let message = if caller_key.is_empty() {
        decision.original_message.clone()
    } else {
        decision.original_message.replace(caller_key, "[redacted]")
    };
    let detail = json!({
        "event":"generation_failed", "request_id":request_id, "task_id":context.task_id,
        "platform":context.manifest.platform_id, "model":context.model.model_id,
        "account_id":context.account.id, "endpoint":endpoint, "status":status, "code":code,
        "category":decision.category, "retry_action":decision.retry_action,
        "account_state":decision.account_state, "cooldown_seconds":decision.cooldown_seconds,
        "error":sanitize_error(&message)
    });
    // 与入口请求日志使用同一 JSON 格式，流开始返回 200 后的失败也必须可追踪。
    eprintln!("{detail}");
    state.store.add_audit_log(
        "protocol",
        "error",
        "生成请求失败".into(),
        detail.to_string(),
        Some(context.task_id.clone()),
    );
}

pub(crate) fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut messages = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        let message = cause.to_string();
        if messages.last() != Some(&message) {
            messages.push(message);
        }
        source = cause.source();
    }
    sanitize_error(&messages.join("; caused by: "))
}

pub(crate) fn sanitize_error(message: &str) -> String {
    // 限制异常响应灌满日志；正常错误链完整保留，超长错误明确标记。
    let bounded: String = message.chars().take(65536).collect();
    let mut result = String::new();
    let mut start = 0;
    for (index, ch) in bounded.char_indices() {
        if index < start || !matches!(ch, '{' | '[') {
            continue;
        }
        let mut parser =
            serde_json::Deserializer::from_str(&bounded[index..]).into_iter::<serde_json::Value>();
        if let Some(Ok(mut value)) = parser.next() {
            result.push_str(&redact_text(&bounded[start..index]));
            redact_json(&mut value);
            result.push_str(&value.to_string());
            start = index + parser.byte_offset();
        }
    }
    result.push_str(&redact_text(&bounded[start..]));
    if bounded.len() < message.len() {
        result.push_str(" [错误过长，已截断至 65536 字符]");
    }
    result
}

fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                let key = key.to_ascii_lowercase().replace(['-', '_'], "");
                if [
                    "authorization",
                    "proxyauthorization",
                    "cookie",
                    "setcookie",
                    "token",
                    "accesstoken",
                    "refreshtoken",
                    "idtoken",
                    "apikey",
                    "password",
                    "secret",
                    "clientsecret",
                    "codeverifier",
                    "prompt",
                    "input",
                    "messages",
                    "request",
                    "requestbody",
                    "body",
                    "b64json",
                    "imagedata",
                    "audiodata",
                ]
                .contains(&key.as_str())
                {
                    *value = json!("[redacted]");
                } else {
                    redact_json(value);
                }
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(redact_json),
        serde_json::Value::String(text) => *text = sanitize_error(text),
        _ => {}
    }
}

fn redact_text(text: &str) -> String {
    use regex::{Captures, Regex};
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<[Regex; 4]> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| [
        Regex::new(r#"(?i)\b(?:authorization|proxy-authorization|cookie|set-cookie)\s*[:=]\s*[^\r\n]+"#).unwrap(),
        Regex::new(r#"(?i)\b(?:access[_-]?token|refresh[_-]?token|id[_-]?token|api[_-]?key|password|client[_-]?secret|secret|token|code[_-]?verifier|prompt|input|request[_-]?body)\b["']?\s*[:=]\s*(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s,;&]+)"#).unwrap(),
        Regex::new(r#"(?i)\bBearer\s+[^\s,;"']+|\b(?:mg_|sk-)[a-zA-Z0-9_-]+|\beyJ[a-zA-Z0-9_.-]{20,}|data:[^\s,]+;base64,[a-zA-Z0-9+/=]+"#).unwrap(),
        Regex::new(r#"(?:https?|socks5h?)://[^\s"'<>]+"#).unwrap(),
    ]);
    let mut result = patterns[3]
        .replace_all(text, |capture: &Captures<'_>| {
            reqwest::Url::parse(&capture[0])
                .map(|mut url| {
                    let _ = url.set_username("");
                    let _ = url.set_password(None);
                    url.set_query(None);
                    url.set_fragment(None);
                    url.to_string()
                })
                .unwrap_or_else(|_| "[redacted-url]".into())
        })
        .into_owned();
    for pattern in &patterns[..3] {
        result = pattern.replace_all(&result, "[redacted]").into_owned();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::public_failure;

    #[test]
    fn model_quota_failure_reports_only_validated_reset_time() {
        let (status, code, public) = public_failure(
            "account_model_unavailable: model_quota_exhausted; model=test; resets_at=2099-11-10T16:51:45.000Z; private-detail",
        );
        assert_eq!((status, code), (429, "rate_limit_exceeded"));
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
}
