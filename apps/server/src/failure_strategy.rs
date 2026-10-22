use mediagen_core::PluginManifest;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FailureDecision {
    pub category: String,
    pub retry_action: String,
    pub account_state: Option<String>,
    pub usage_consumed: bool,
    pub same_error_threshold: u32,
    pub threshold_cooldown_seconds: i64,
    pub cooldown_from_message: bool,
    pub cooldown_seconds: i64,
    pub original_message: String,
}

impl FailureDecision {
    pub(crate) fn verification_required(&self) -> bool {
        self.category == "verification_required"
            || self.account_state.as_deref() == Some("verification_required")
    }

    pub(crate) fn formatted_message(&self) -> String {
        if self.category == "unknown" {
            return self.original_message.clone();
        }
        format!("[{}] {}", self.category, self.original_message)
    }

    // HTTP 只暴露 Realove 能稳定消费的安全类别，原始消息仍留在受控任务事件中。
    pub(crate) fn response_error(&self) -> (u16, &'static str) {
        match self.category.as_str() {
            "insufficient_quota" => (429, "insufficient_quota"),
            "permission_denied" => (403, "account_permission_denied"),
            "adult_sensual_policy_blocked" => (422, "adult_sensual_policy_blocked"),
            "unsafe_other" => (422, "unsafe_other"),
            "account_model_unavailable" => (409, "account_model_unavailable"),
            "invalid_request" => (422, "invalid_request"),
            "account_rate_limited" | "quota_exhausted" => (429, "rate_limited"),
            "temporary_unavailable" | "transient_network" | "platform_busy" | "login_expired" => {
                (503, "temporary_unavailable")
            }
            _ => (502, "plugin_execution_failed"),
        }
    }

    pub(crate) fn should_retry_same_account(&self, failures: u32) -> bool {
        self.retry_action == "same_account_retry" && failures < self.same_error_threshold
    }

    pub(crate) fn should_failover(&self) -> bool {
        self.retry_action == "failover"
    }

    pub(crate) fn should_cooldown_after_retry_exhausted(&self) -> bool {
        self.retry_action == "same_account_retry" && self.threshold_cooldown_seconds > 0
    }

    pub(crate) fn cooldown_seconds(&self, message: &str, fallback: i64) -> i64 {
        if self.cooldown_from_message {
            return seconds_from_message(message).unwrap_or(fallback).max(1);
        }
        if self.cooldown_seconds > 0 {
            self.cooldown_seconds
        } else {
            fallback.max(1)
        }
    }

    pub(crate) fn cooldown_seconds_from_message(&self, message: &str) -> Option<i64> {
        self.cooldown_from_message
            .then(|| seconds_from_message(message))
            .flatten()
            .map(|seconds| seconds.max(1))
    }
}

pub(crate) fn classify_failure(manifest: &PluginManifest, message: &str) -> FailureDecision {
    let fallback = FailureDecision {
        category: "unknown".to_string(),
        retry_action: "fail".to_string(),
        account_state: None,
        usage_consumed: false,
        same_error_threshold: 1,
        threshold_cooldown_seconds: 0,
        cooldown_from_message: false,
        cooldown_seconds: 0,
        original_message: message.to_string(),
    };
    let Some(strategy) = manifest
        .extra
        .get("failure_strategy")
        .and_then(Value::as_array)
    else {
        return fallback;
    };
    strategy
        .iter()
        .find(|rule| rule_matches(rule, message))
        .map(|rule| decision_from_rule(rule, message))
        .unwrap_or(fallback)
}

fn rule_matches(rule: &Value, message: &str) -> bool {
    let text = message.to_ascii_lowercase();
    contains_any(rule.get("contains_any"), &text) || contains_any(rule.get("error_codes"), &text)
}

fn contains_any(value: Option<&Value>, message: &str) -> bool {
    value
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| contains_text(item, message)))
}

fn contains_text(item: &Value, message: &str) -> bool {
    item.as_str()
        .map(|needle| message.contains(&needle.to_ascii_lowercase()))
        .unwrap_or(false)
}

fn decision_from_rule(rule: &Value, message: &str) -> FailureDecision {
    FailureDecision {
        category: text_field(rule, "category").unwrap_or_else(|| "unknown".to_string()),
        retry_action: text_field(rule, "retry_action").unwrap_or_else(|| "fail".to_string()),
        account_state: text_field(rule, "account_state"),
        usage_consumed: rule
            .get("usage_consumed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        same_error_threshold: u32_field(rule, "same_error_threshold").unwrap_or(1).max(1),
        threshold_cooldown_seconds: i64_field(rule, "threshold_cooldown_seconds").unwrap_or(0),
        cooldown_from_message: rule
            .get("cooldown_from_message")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        cooldown_seconds: i64_field(rule, "cooldown_seconds").unwrap_or(0),
        original_message: message.to_string(),
    }
}

fn seconds_from_message(message: &str) -> Option<i64> {
    let text = message.to_ascii_lowercase();
    let mut total = 0i64;
    let mut found = false;
    for (index, ch) in text.char_indices() {
        if !ch.is_ascii_digit() {
            continue;
        }
        if index > 0
            && text[..index]
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_digit())
        {
            continue;
        }
        let Some((value, rest)) = number_and_unit_tail(&text[index..]) else {
            continue;
        };
        if let Some(multiplier) = duration_unit_multiplier(rest) {
            total = total.saturating_add(value.saturating_mul(multiplier));
            found = true;
        }
    }
    found.then_some(total)
}

fn number_and_unit_tail(text: &str) -> Option<(i64, &str)> {
    let digits_end = text
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    let value = text[..digits_end].parse().ok()?;
    let rest = text[digits_end..].trim_start_matches(duration_separator);
    Some((value, rest))
}

fn duration_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ':' | ',' | '.' | '，' | '。' | '约' | '~' | '～')
}

fn duration_unit_multiplier(text: &str) -> Option<i64> {
    [
        ("seconds", 1),
        ("second", 1),
        ("secs", 1),
        ("sec", 1),
        ("秒", 1),
        ("minutes", 60),
        ("minute", 60),
        ("mins", 60),
        ("min", 60),
        ("分钟", 60),
        ("hours", 3600),
        ("hour", 3600),
        ("hrs", 3600),
        ("hr", 3600),
        ("小时", 3600),
        ("days", 86400),
        ("day", 86400),
        ("天", 86400),
    ]
    .into_iter()
    .find_map(|(unit, multiplier)| text.starts_with(unit).then_some(multiplier))
}

fn text_field(rule: &Value, key: &str) -> Option<String> {
    rule.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn u32_field(rule: &Value, key: &str) -> Option<u32> {
    rule.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn i64_field(rule: &Value, key: &str) -> Option<i64> {
    rule.get(key).and_then(Value::as_i64)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mediagen_core::PluginManifest;
    use serde_json::json;

    use super::classify_failure;

    #[test]
    fn classifies_account_state_from_manifest_rules() {
        let manifest = manifest_with_rules(json!([
            {
                "contains_any": ["rate limit", "请求过于频繁"],
                "category": "account_rate_limited",
                "retry_action": "failover",
                "account_state": "cooldown",
                "usage_consumed": false
            }
        ]));

        let decision = classify_failure(&manifest, "HTTP 429 rate limit");

        assert_eq!(decision.category, "account_rate_limited");
        assert_eq!(decision.retry_action, "failover");
        assert_eq!(decision.account_state.as_deref(), Some("cooldown"));
        assert_eq!(decision.same_error_threshold, 1);
    }

    #[test]
    fn treats_verification_category_as_takeover_required() {
        let manifest = manifest_with_rules(json!([
            {
                "contains_any": ["需要验证"],
                "category": "verification_required",
                "account_state": "verification_required"
            }
        ]));

        let decision = classify_failure(&manifest, "账号需要验证");

        assert!(decision.verification_required());
        assert!(decision
            .formatted_message()
            .contains("verification_required"));
    }

    #[test]
    fn falls_back_when_no_rule_matches() {
        let decision = classify_failure(&manifest_with_rules(json!([])), "boom");

        assert_eq!(decision.category, "unknown");
        assert_eq!(decision.formatted_message(), "boom");
    }

    #[test]
    fn maps_failure_categories_to_stable_http_errors() {
        let cases = [
            ("permission_denied", (403, "account_permission_denied")),
            ("insufficient_quota", (429, "insufficient_quota")),
            (
                "adult_sensual_policy_blocked",
                (422, "adult_sensual_policy_blocked"),
            ),
            ("unsafe_other", (422, "unsafe_other")),
            ("invalid_request", (422, "invalid_request")),
            (
                "account_model_unavailable",
                (409, "account_model_unavailable"),
            ),
            ("temporary_unavailable", (503, "temporary_unavailable")),
            ("account_rate_limited", (429, "rate_limited")),
            ("unknown", (502, "plugin_execution_failed")),
        ];
        for (category, expected) in cases {
            let decision = classify_failure(
                &manifest_with_rules(json!([{
                    "contains_any": ["matched"],
                    "category": category
                }])),
                "matched",
            );
            assert_eq!(decision.response_error(), expected);
        }
    }

    #[test]
    fn transient_cdp_failure_never_marks_login_expired() {
        let decision = classify_failure(
            &manifest_with_rules(json!([])),
            r#"cdp error: {"code":-32000,"message":"Inspected target navigated or closed"}"#,
        );

        assert_eq!(decision.category, "unknown");
        assert_eq!(decision.account_state, None);
    }

    #[test]
    fn reads_same_account_retry_thresholds() {
        let manifest = manifest_with_rules(json!([
            {
                "contains_any": ["busy"],
                "category": "platform_busy",
                "retry_action": "same_account_retry",
                "same_error_threshold": 3,
                "threshold_cooldown_seconds": 600
            }
        ]));

        let decision = classify_failure(&manifest, "platform busy");

        assert_eq!(decision.same_error_threshold, 3);
        assert_eq!(decision.threshold_cooldown_seconds, 600);
    }

    #[test]
    fn reads_cooldown_from_message_or_rule() {
        let manifest = manifest_with_rules(json!([
            {
                "contains_any": ["rate"],
                "category": "account_rate_limited",
                "retry_action": "failover",
                "account_state": "cooldown",
                "cooldown_from_message": true,
                "cooldown_seconds": -1
            }
        ]));

        let decision = classify_failure(&manifest, "rate limited, please wait for 3 minutes");

        assert_eq!(
            decision.cooldown_seconds("please wait for 3 minutes", 300),
            180
        );
        assert_eq!(decision.cooldown_seconds("rate", 300), 300);
    }

    #[test]
    fn reads_mixed_hours_minutes_days_from_message() {
        let decision = classify_failure(
            &manifest_with_rules(json!([{
                "contains_any": ["quota"],
                "category": "quota_exhausted",
                "account_state": "daily_quota_exhausted",
                "cooldown_from_message": true
            }])),
            "quota",
        );

        assert_eq!(
            decision.cooldown_seconds_from_message("limit resets in 4 hours and 35 minutes"),
            Some(16_500)
        );
        assert_eq!(
            decision.cooldown_seconds_from_message("请在 1 天 2小时30分钟 后重试"),
            Some(95_400)
        );
    }

    fn manifest_with_rules(rules: serde_json::Value) -> PluginManifest {
        let mut extra = HashMap::new();
        extra.insert("failure_strategy".to_string(), rules);
        PluginManifest {
            platform_id: "fake".to_string(),
            display_name: "Fake".to_string(),
            sort_order: 1,
            adapter_mode: mediagen_core::AdapterMode::Browser,
            login_url: String::new(),
            workspace_url: String::new(),
            models: Vec::new(),
            tiers: HashMap::new(),
            flows: HashMap::new(),
            extra,
        }
    }
}
