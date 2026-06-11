//! 人工验证策略工具。
//!
//! 这里不写任何平台规则，只读取插件 manifest 里声明的验证触发词、忽略词和 DOM 选择器。

use crate::api_server::ImageGenContext;
use crate::types::{PluginManifest, VerificationPolicy};
use crate::AppState;

/// 验证码消失后随机延迟下限（毫秒）
const CAPTCHA_CLEARED_MIN_MS: u64 = 3_000;
/// 验证码消失后随机延迟上限（毫秒）
const CAPTCHA_CLEARED_MAX_MS: u64 = 5_000;

/// 判断错误字符串是否命中插件声明的人工验证触发策略。
pub(crate) fn error_suggests_verification(manifest: Option<&PluginManifest>, msg: &str) -> bool {
    let Some(policy) = manifest.and_then(|m| m.verification.as_ref()) else {
        return false;
    };
    let lower = msg.to_lowercase();
    if contains_any(&lower, &policy.ignore_contains_any) {
        return false;
    }
    contains_any(&lower, &policy.trigger_contains_any)
}

/// 克隆当前平台的验证策略，避免调用方持有 PluginLoader 内部引用跨异步边界。
pub(crate) fn cloned_policy(state: &AppState, ctx: &ImageGenContext) -> Option<VerificationPolicy> {
    state
        .plugin_loader
        .get_plugin(&ctx.platform)
        .and_then(|manifest| manifest.verification.clone())
}

/// 把插件声明的验证码选择器转换为浏览器可执行的轻量探测脚本。
pub(crate) fn captcha_probe_script(selectors: &[String]) -> Option<String> {
    if selectors.is_empty() {
        return None;
    }
    let json = serde_json::to_string(selectors).ok()?;
    Some(format!(
        "JSON.stringify({{ present: {json}.some((selector) => document.querySelector(selector) !== null) }})"
    ))
}

/// 读取插件配置的验证码消失后关窗延迟；配置缺失或不合法时使用通用默认值。
pub(crate) fn close_delay_bounds(policy: &VerificationPolicy) -> (u64, u64) {
    match policy.close_after_cleared_ms.as_slice() {
        [min_ms, max_ms, ..] if max_ms >= min_ms => (*min_ms, *max_ms),
        _ => (CAPTCHA_CLEARED_MIN_MS, CAPTCHA_CLEARED_MAX_MS),
    }
}

fn contains_any(text: &str, needles: &[String]) -> bool {
    needles
        .iter()
        .map(|needle| needle.to_lowercase())
        .any(|needle| !needle.is_empty() && text.contains(&needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_suggests_verification_uses_manifest_policy() {
        let manifest = PluginManifest {
            platform_id: "demo".to_string(),
            display_name: "demo".to_string(),
            sort_order: 1000,
            adapter_mode: crate::types::AdapterMode::Browser,
            login_url: "https://example.com".to_string(),
            workspace_url: "https://example.com".to_string(),
            models: Vec::new(),
            tiers: std::collections::HashMap::new(),
            watermark: None,
            flows: std::collections::HashMap::new(),
            registration: None,
            login_detection: None,
            login_check: None,
            userinfo_script: None,
            verification_fallback: None,
            browser_lifecycle: crate::types::BrowserLifecycle::PerTask,
            image_capabilities: crate::types::ImageCapabilities::default(),
            failure_policy: Vec::new(),
            verification: Some(VerificationPolicy {
                trigger_contains_any: vec!["rate limited".to_string(), "需要验证".to_string()],
                ignore_contains_any: vec![
                    "rate_limit_exceeded".to_string(),
                    "生成过于频繁".to_string(),
                ],
                captcha_selectors: vec![".captcha".to_string()],
                timeout_seconds: Some(600),
                close_after_cleared_ms: vec![3000, 5000],
            }),
        };

        assert!(error_suggests_verification(
            Some(&manifest),
            "平台错误: rate limited"
        ));
        assert!(error_suggests_verification(Some(&manifest), "需要验证"));
        assert!(!error_suggests_verification(Some(&manifest),
            "平台错误: 生成过于频繁: You're generating images too quickly. Please wait for 8 minutes before generating more images."
        ));
        assert!(!error_suggests_verification(
            Some(&manifest),
            "rate_limit_exceeded"
        ));
        assert!(!error_suggests_verification(Some(&manifest), "内容违规"));
        assert!(!error_suggests_verification(None, "平台错误: rate limited"));
    }
}
