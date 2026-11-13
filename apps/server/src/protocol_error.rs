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
    for (prefix, status, code) in [
        (
            "no_available_account_for_model: ",
            403,
            "no_available_account_for_model",
        ),
        ("insufficient_quota: ", 429, "insufficient_quota"),
    ] {
        if let Some(detail) = message.strip_prefix(prefix) {
            return (status, code, detail.to_string());
        }
    }
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
        return (429, "insufficient_quota", detail);
    }
    if message.contains("account_model_unavailable") {
        return (
            409,
            "account_model_unavailable",
            "当前账号不支持所选模型，请刷新账号模型目录；已有会话需恢复原账号能力".into(),
        );
    }
    if message.contains("recovery_exhausted:") && message.contains("连续三次连接失败") {
        return (
            409,
            "recovery_exhausted",
            "原请求连续三次连接失败，已停止自动恢复；请检查网络并核对原任务".into(),
        );
    }
    for (code, detail) in [
        (
            "conversation_unavailable",
            "上游已无法加载原对话，已停止恢复；请新建请求",
        ),
        (
            "recovery_exhausted",
            "原任务超过恢复时限，已停止自动恢复；请先核对上游原对话",
        ),
        (
            "generation_failed",
            "上游明确返回生成失败，已停止恢复；请检查请求或新建任务",
        ),
    ] {
        if message.contains(&format!("{code}:")) {
            return (409, code, detail.into());
        }
    }
    let lower = message.to_lowercase();
    if [
        "proxyconnect",
        "socks error",
        "tunnel error",
        "proxy_unavailable:",
    ]
    .iter()
    .any(|kind| lower.contains(kind))
    {
        return (
            409,
            "proxy_unavailable",
            "配置的代理连接失败，已停止本次请求；请检查代理后重新提交".into(),
        );
    }
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
    if message.contains("invalid_input") {
        return (
            400,
            "invalid_input",
            if message.contains("invalid_input: 附件文件名无效") {
                "附件文件名无效，请检查文件名"
            } else {
                "请求输入格式无效，请检查输入内容与附件格式"
            }
            .into(),
        );
    }
    if message.contains("missing_input") {
        return (
            400,
            "missing_input",
            "请求缺少必填输入，请检查输入内容与附件".into(),
        );
    }
    if message.contains("unsupported_") {
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
#[path = "protocol_error_tests.rs"]
mod tests;
