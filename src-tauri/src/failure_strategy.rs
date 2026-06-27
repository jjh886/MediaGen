use crate::account_manager::extract_cooldown_duration_hint;
use crate::error::AppError;
use crate::types::{FailureCategory, FailureRetryAction, LoginState, PluginManifest, StepResult};

const DEFAULT_FAILURE_WINDOW_SECONDS: i64 = 30 * 60;
const DEFAULT_SAME_ERROR_THRESHOLD: i32 = 3;
const DEFAULT_THRESHOLD_COOLDOWN_SECONDS: i64 = 10 * 60;

#[derive(Debug, Clone)]
pub struct FailureDecision {
    pub category: FailureCategory,
    pub retry_action: FailureRetryAction,
    pub account_state: Option<LoginState>,
    pub cooldown_hint: Option<i64>,
    pub usage_consumed: Option<bool>,
    pub error_code: Option<String>,
    pub message: String,
    pub fingerprint: String,
    pub same_error_threshold: i32,
    pub threshold_cooldown_seconds: i64,
    pub failure_window_seconds: i64,
}

pub fn classify(
    manifest: Option<&PluginManifest>,
    error: &AppError,
    step_results: &[StepResult],
) -> FailureDecision {
    let signal = FailureSignal::from(error, step_results);
    if let Some(rule) = manifest.and_then(|m| match_manifest_strategy(m, &signal)) {
        return decision_from_rule(rule, signal);
    }
    fallback_decision(signal)
}

pub fn classify_account_state(
    manifest: Option<&PluginManifest>,
    error_msg: &str,
) -> Option<(LoginState, i64)> {
    let signal = FailureSignal::from_message(error_msg);
    let rule = manifest.and_then(|m| match_manifest_strategy(m, &signal))?;
    let state = rule.account_state?;
    Some((
        state,
        cooldown_hint(rule.cooldown_from_message, rule.cooldown_seconds, &signal),
    ))
}

pub fn default_failure_window_seconds() -> i64 {
    DEFAULT_FAILURE_WINDOW_SECONDS
}

struct FailureSignal {
    message: String,
    lower_message: String,
    error_code: Option<String>,
    retryable: Option<bool>,
    usage_consumed: Option<bool>,
}

struct FallbackPlan {
    category: FailureCategory,
    retry_action: FailureRetryAction,
    account_state: Option<LoginState>,
    cooldown_hint: Option<i64>,
    usage_consumed: Option<bool>,
    same_error_threshold: Option<i32>,
    threshold_cooldown_seconds: Option<i64>,
}

impl FailureSignal {
    fn from(error: &AppError, step_results: &[StepResult]) -> Self {
        let message = combined_message(&error.to_string(), step_results);
        let last_data = last_failed_data(step_results);
        Self {
            lower_message: message.to_lowercase(),
            message,
            error_code: extract_error_code(last_data),
            retryable: last_data.and_then(|data| data.get("retryable").and_then(|v| v.as_bool())),
            usage_consumed: last_data
                .and_then(|data| data.get("usage_consumed").and_then(|v| v.as_bool())),
        }
    }

    fn from_message(message: &str) -> Self {
        Self {
            message: message.to_string(),
            lower_message: message.to_lowercase(),
            error_code: None,
            retryable: None,
            usage_consumed: None,
        }
    }
}

fn combined_message(error: &str, step_results: &[StepResult]) -> String {
    let mut parts = vec![error.to_string()];
    for result in step_results.iter().rev().take(3) {
        if let Some(err) = result.error.as_deref().filter(|v| !v.trim().is_empty()) {
            parts.push(err.to_string());
        }
        if let Some(msg) = result.data.get("message").and_then(|v| v.as_str()) {
            parts.push(msg.to_string());
        }
        if let Some(code) = result.data.get("error_code").and_then(|v| v.as_str()) {
            parts.push(code.to_string());
        }
    }
    parts.join(" ")
}

fn last_failed_data(step_results: &[StepResult]) -> Option<&serde_json::Value> {
    step_results
        .iter()
        .rev()
        .find(|result| !result.success)
        .map(|result| &result.data)
}

fn extract_error_code(data: Option<&serde_json::Value>) -> Option<String> {
    data.and_then(|data| data.get("error_code").and_then(|v| v.as_str()))
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
}

fn match_manifest_strategy<'a>(
    manifest: &'a PluginManifest,
    signal: &FailureSignal,
) -> Option<&'a crate::types::FailureStrategyRule> {
    manifest.failure_strategy.iter().find(|rule| {
        error_code_matches(rule, signal) || contains_any(&signal.lower_message, &rule.contains_any)
    })
}

fn error_code_matches(rule: &crate::types::FailureStrategyRule, signal: &FailureSignal) -> bool {
    let Some(code) = signal.error_code.as_deref() else {
        return false;
    };
    rule.error_codes
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(code))
}

fn contains_any(text: &str, needles: &[String]) -> bool {
    needles
        .iter()
        .map(|needle| needle.to_lowercase())
        .any(|needle| !needle.is_empty() && text.contains(&needle))
}

fn decision_from_rule(
    rule: &crate::types::FailureStrategyRule,
    signal: FailureSignal,
) -> FailureDecision {
    let cooldown = cooldown_hint(rule.cooldown_from_message, rule.cooldown_seconds, &signal);
    build_decision(
        rule.category,
        rule.retry_action,
        rule.account_state,
        Some(cooldown),
        rule.usage_consumed.or(signal.usage_consumed),
        signal,
        rule.same_error_threshold,
        rule.threshold_cooldown_seconds,
    )
}

fn fallback_decision(signal: FailureSignal) -> FailureDecision {
    let plan = fallback_plan(&signal);
    build_decision(
        plan.category,
        plan.retry_action,
        plan.account_state,
        plan.cooldown_hint,
        plan.usage_consumed,
        signal,
        plan.same_error_threshold,
        plan.threshold_cooldown_seconds,
    )
}

fn fallback_plan(signal: &FailureSignal) -> FallbackPlan {
    content_or_invalid_plan(signal)
        .or_else(|| login_expired_plan(signal))
        .or_else(|| quota_plan(signal))
        .or_else(|| rate_limit_plan(signal))
        .or_else(|| transient_retry_plan(signal))
        .unwrap_or_else(|| unknown_plan(signal))
}

fn content_or_invalid_plan(signal: &FailureSignal) -> Option<FallbackPlan> {
    is_content_or_invalid(&signal.lower_message).then(|| FallbackPlan {
        category: FailureCategory::InvalidRequest,
        retry_action: FailureRetryAction::Fail,
        account_state: None,
        cooldown_hint: None,
        usage_consumed: Some(false),
        same_error_threshold: None,
        threshold_cooldown_seconds: None,
    })
}

fn login_expired_plan(signal: &FailureSignal) -> Option<FallbackPlan> {
    is_login_expired(&signal.lower_message).then(|| FallbackPlan {
        category: FailureCategory::LoginExpired,
        retry_action: FailureRetryAction::Failover,
        account_state: Some(LoginState::LoginExpired),
        cooldown_hint: Some(0),
        usage_consumed: Some(false),
        same_error_threshold: None,
        threshold_cooldown_seconds: None,
    })
}

fn quota_plan(signal: &FailureSignal) -> Option<FallbackPlan> {
    is_quota_error(&signal.lower_message).then(|| FallbackPlan {
        category: FailureCategory::QuotaExhausted,
        retry_action: FailureRetryAction::Failover,
        account_state: Some(LoginState::DailyQuotaExhausted),
        cooldown_hint: Some(0),
        usage_consumed: Some(false),
        same_error_threshold: None,
        threshold_cooldown_seconds: None,
    })
}

fn rate_limit_plan(signal: &FailureSignal) -> Option<FallbackPlan> {
    is_rate_limited(&signal.lower_message).then(|| FallbackPlan {
        category: FailureCategory::AccountRateLimited,
        retry_action: FailureRetryAction::Failover,
        account_state: Some(LoginState::Cooldown),
        cooldown_hint: Some(extract_cooldown_duration_hint(&signal.message).unwrap_or(-1)),
        usage_consumed: Some(false),
        same_error_threshold: None,
        threshold_cooldown_seconds: None,
    })
}

fn transient_retry_plan(signal: &FailureSignal) -> Option<FallbackPlan> {
    (is_browser_or_network(&signal.lower_message) || signal.retryable == Some(true)).then(|| {
        FallbackPlan {
            category: FailureCategory::BrowserTransient,
            retry_action: FailureRetryAction::SameAccountRetry,
            account_state: None,
            cooldown_hint: None,
            usage_consumed: Some(false),
            same_error_threshold: Some(DEFAULT_SAME_ERROR_THRESHOLD),
            threshold_cooldown_seconds: Some(DEFAULT_THRESHOLD_COOLDOWN_SECONDS),
        }
    })
}

fn unknown_plan(signal: &FailureSignal) -> FallbackPlan {
    FallbackPlan {
        category: FailureCategory::Unknown,
        retry_action: FailureRetryAction::Fail,
        account_state: None,
        cooldown_hint: None,
        usage_consumed: signal.usage_consumed,
        same_error_threshold: None,
        threshold_cooldown_seconds: None,
    }
}

fn build_decision(
    category: FailureCategory,
    retry_action: FailureRetryAction,
    account_state: Option<LoginState>,
    cooldown_hint: Option<i64>,
    usage_consumed: Option<bool>,
    signal: FailureSignal,
    same_error_threshold: Option<i32>,
    threshold_cooldown_seconds: Option<i64>,
) -> FailureDecision {
    let fingerprint = fingerprint(category, signal.error_code.as_deref(), &signal.message);
    FailureDecision {
        category,
        retry_action,
        account_state,
        cooldown_hint,
        usage_consumed,
        error_code: signal.error_code,
        message: signal.message,
        fingerprint,
        same_error_threshold: same_error_threshold.unwrap_or(DEFAULT_SAME_ERROR_THRESHOLD),
        threshold_cooldown_seconds: threshold_cooldown_seconds
            .unwrap_or(DEFAULT_THRESHOLD_COOLDOWN_SECONDS),
        failure_window_seconds: DEFAULT_FAILURE_WINDOW_SECONDS,
    }
}

fn cooldown_hint(from_message: bool, configured: Option<i64>, signal: &FailureSignal) -> i64 {
    if from_message {
        return extract_cooldown_duration_hint(&signal.message)
            .or(configured)
            .unwrap_or(0);
    }
    configured.unwrap_or(0)
}

fn fingerprint(category: FailureCategory, code: Option<&str>, message: &str) -> String {
    let code = code.unwrap_or("").trim();
    if !code.is_empty() {
        return format!("{category:?}:{code}");
    }
    let short: String = message.chars().take(80).collect();
    format!("{category:?}:{short}")
}

fn is_content_or_invalid(text: &str) -> bool {
    [
        "内容违规",
        "content_policy",
        "safety",
        "invalid",
        "bad request",
        "unsupported",
        "model_not_found",
        "参数",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn is_login_expired(text: &str) -> bool {
    ["login_expired", "登录", "logout", "expired", "from_logout"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn is_quota_error(text: &str) -> bool {
    ["quota", "额度耗尽", "free plan limit", "limit resets in"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn is_rate_limited(text: &str) -> bool {
    [
        "rate_limit",
        "rate limit",
        "请求过于频繁",
        "生成过于频繁",
        "http 429",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn is_browser_or_network(text: &str) -> bool {
    [
        "浏览器错误",
        "browser",
        "timeout",
        "timed out",
        "tab closed",
        "session closed",
        "underlying connection is closed",
        "err_connection",
        "err_tunnel",
        "err_proxy",
        "network",
        "proxy",
        "socket",
        "connection reset",
        "connection refused",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FailureStrategyRule;

    #[test]
    fn strategy_prefers_error_code() {
        let manifest = PluginManifest {
            platform_id: "demo".to_string(),
            display_name: "demo".to_string(),
            sort_order: 1000,
            adapter_mode: crate::types::AdapterMode::Browser,
            login_url: String::new(),
            workspace_url: String::new(),
            models: vec![],
            tiers: Default::default(),
            watermark: None,
            flows: Default::default(),
            registration: None,
            login_detection: None,
            login_check: None,
            userinfo_script: None,
            verification_fallback: None,
            browser_lifecycle: crate::types::BrowserLifecycle::PerTask,
            image_capabilities: Default::default(),
            failure_policy: vec![],
            failure_strategy: vec![FailureStrategyRule {
                error_codes: vec!["rate_limit_exceeded".to_string()],
                category: FailureCategory::AccountRateLimited,
                retry_action: FailureRetryAction::Failover,
                account_state: Some(LoginState::Cooldown),
                cooldown_seconds: Some(30),
                ..Default::default()
            }],
            verification: None,
        };
        let steps = vec![StepResult {
            success: false,
            data: serde_json::json!({"error_code": "rate_limit_exceeded"}),
            error: Some("too quick".to_string()),
        }];
        let decision = classify(
            Some(&manifest),
            &AppError::PlatformError("failed".to_string()),
            &steps,
        );
        assert_eq!(decision.retry_action, FailureRetryAction::Failover);
        assert_eq!(decision.account_state, Some(LoginState::Cooldown));
    }
}
