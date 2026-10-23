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

#[cfg(test)]
mod tests {
    use super::public_failure;

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
}
