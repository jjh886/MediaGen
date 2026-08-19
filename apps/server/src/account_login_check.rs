use mediagen_browser_gateway::{
    BrowserSessionCreateRequest, BrowserSessionPurpose, BrowserSessionResponse,
};
use mediagen_core::PluginManifest;
use mediagen_plugin_runtime::run_js_step;
use serde_json::{json, Value};
use std::path::Path;
use tokio::time::{sleep, Duration};

use crate::account_browser_profile::browser_metadata_from_account;
use crate::account_login::{
    configured_script_files, login_detection_config, profile_from_value, run_login_script_value,
    AccountLoginProfile, LoginDetectionConfig,
};
use crate::flow_templates::collect_script_files;
use crate::server_plugin_host::ServerPluginHost;
use crate::store::AccountRecord;
use crate::ServerState;

pub(crate) enum AccountLoginCheckOutcome {
    Available(AccountLoginProfile),
    Unavailable {
        login_state: String,
        profile: AccountLoginProfile,
        cooldown_seconds: Option<i64>,
    },
    Expired(String),
    Inconclusive(String),
}

/// 检查已保存账号的登录态；只有插件明确返回过期时才停用账号。
pub(crate) async fn check_saved_account_login(
    state: &ServerState,
    account: &AccountRecord,
) -> AccountLoginCheckOutcome {
    let Some(manifest) = state.plugin_loader.get_plugin(&account.platform) else {
        return AccountLoginCheckOutcome::Inconclusive(format!(
            "平台插件 '{}' 未找到",
            account.platform
        ));
    };
    if let Some(outcome) = check_saved_account_with_flow(state, manifest, account).await {
        return outcome;
    }
    check_saved_account_with_browser(state, manifest, account).await
}

async fn check_saved_account_with_flow(
    state: &ServerState,
    manifest: &PluginManifest,
    account: &AccountRecord,
) -> Option<AccountLoginCheckOutcome> {
    let flow = manifest.flows.get("check_account")?;
    let host = ServerPluginHost::new_for_account(account);
    let mut context = json!({
        "platform": account.platform,
        "account": account,
        "account_id": account.id,
        "profile_dir": account.profile_dir,
    });
    let mut results = Vec::with_capacity(flow.steps.len());
    for step in &flow.steps {
        let result =
            match run_check_account_step(state, manifest, account, step, &context, &host).await {
                Ok(result) => result,
                Err(error) => return Some(classify_flow_error(&error)),
            };
        save_flow_result(step, &mut context, &result);
        results.push(result);
    }
    Some(classify_flow_results(&results))
}

async fn check_saved_account_with_browser(
    state: &ServerState,
    manifest: &PluginManifest,
    account: &AccountRecord,
) -> AccountLoginCheckOutcome {
    let response = match open_login_check_session(state, manifest, account).await {
        Ok(response) => response,
        Err(error) => return AccountLoginCheckOutcome::Inconclusive(error),
    };
    let outcome = check_browser_session(state, manifest, &response.session).await;
    // 检查使用独立 Tab；关闭它不能影响同账号正在生成的其他 Tab。
    let _ = state
        .browser_gateway
        .cancel_session(&response.session.id)
        .await;
    outcome
}

async fn open_login_check_session(
    state: &ServerState,
    manifest: &PluginManifest,
    account: &AccountRecord,
) -> Result<BrowserSessionResponse, String> {
    let request = login_check_request(manifest, account);
    let profile_lock = state
        .browser_gateway
        .profile_open_lock(&account.profile_dir)
        .map_err(|error| error.to_string())?;
    let _profile_guard = profile_lock.lock().await;
    if let Some(response) = state
        .browser_gateway
        .create_profile_tab_session(request.clone())
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(response);
    }
    state
        .browser_gateway
        .create_session(request)
        .await
        .map_err(|error| error.to_string())
}

async fn check_browser_session(
    state: &ServerState,
    manifest: &PluginManifest,
    session: &mediagen_browser_gateway::BrowserSession,
) -> AccountLoginCheckOutcome {
    match login_detection_config(manifest) {
        Ok(LoginDetectionConfig::JsScript {
            script_file,
            script_files,
            await_promise,
        }) => match configured_script_files(script_file, script_files) {
            Ok(references) => {
                check_browser_script_with_retry(state, session, &references, await_promise).await
            }
            Err(error) => AccountLoginCheckOutcome::Inconclusive(error),
        },
        Ok(_) => AccountLoginCheckOutcome::Inconclusive(
            "该登录检测方式无法可靠区分退出登录与临时检查失败".to_string(),
        ),
        Err(error) => AccountLoginCheckOutcome::Inconclusive(error),
    }
}

async fn check_browser_script_with_retry(
    state: &ServerState,
    session: &mediagen_browser_gateway::BrowserSession,
    script_files: &[String],
    await_promise: bool,
) -> AccountLoginCheckOutcome {
    let mut last = AccountLoginCheckOutcome::Inconclusive("登录页面尚未准备完成".to_string());
    for _ in 0..6 {
        match run_login_script_value(state, session, script_files, await_promise).await {
            Ok(value) => {
                let outcome = classify_browser_login_value(&value);
                if matches!(outcome, AccountLoginCheckOutcome::Available(_)) {
                    return outcome;
                }
                last = outcome;
            }
            Err(error) => last = AccountLoginCheckOutcome::Inconclusive(error),
        }
        sleep(Duration::from_millis(800)).await;
    }
    last
}

/// 与 aiduanju 的流程分类保持一致，并允许从多步骤结果的 health 字段读取状态。
fn classify_flow_results(results: &[Value]) -> AccountLoginCheckOutcome {
    for value in results.iter().rev() {
        if let Some(outcome) = classify_flow_value(value) {
            return outcome;
        }
        if let Some(outcome) = value.get("health").and_then(classify_flow_value) {
            return outcome;
        }
    }
    AccountLoginCheckOutcome::Inconclusive("账号检查脚本没有返回登录状态".to_string())
}

fn classify_flow_value(value: &Value) -> Option<AccountLoginCheckOutcome> {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match status {
        "available" => {
            let mut profile = profile_from_value(value);
            profile.logged_in = true;
            return Some(AccountLoginCheckOutcome::Available(profile));
        }
        "cooldown"
        | "daily_quota_exhausted"
        | "total_quota_exhausted"
        | "verification_required" => {
            return Some(AccountLoginCheckOutcome::Unavailable {
                login_state: status.to_string(),
                profile: profile_from_value(value),
                cooldown_seconds: flow_cooldown_seconds(value),
            });
        }
        "login_expired" => return Some(AccountLoginCheckOutcome::Expired(flow_reason(value))),
        "inconclusive" => return Some(AccountLoginCheckOutcome::Inconclusive(flow_reason(value))),
        _ => {}
    }
    value
        .get("logged_in")
        .and_then(Value::as_bool)
        .map(|logged_in| {
            if logged_in {
                AccountLoginCheckOutcome::Available(profile_from_value(value))
            } else {
                AccountLoginCheckOutcome::Expired(flow_reason(value))
            }
        })
}

fn classify_browser_login_value(value: &Value) -> AccountLoginCheckOutcome {
    if let Some(outcome) = classify_flow_value(value) {
        return outcome;
    }
    let profile = profile_from_value(value);
    if profile.logged_in {
        return AccountLoginCheckOutcome::Available(profile);
    }
    // 旧插件的 false 可能来自风控、网络或页面未加载，不能据此停用真实可用账号。
    AccountLoginCheckOutcome::Inconclusive(flow_reason(value))
}

fn classify_flow_error(message: &str) -> AccountLoginCheckOutcome {
    if let Some(reason) = message.split("LOGIN_EXPIRED:").nth(1) {
        return AccountLoginCheckOutcome::Expired(reason.trim().to_string());
    }
    AccountLoginCheckOutcome::Inconclusive(message.to_string())
}

fn flow_reason(value: &Value) -> String {
    value
        .get("reason")
        .and_then(Value::as_str)
        .filter(|reason| !reason.trim().is_empty())
        .unwrap_or("账号登录检查结果不确定")
        .to_string()
}

fn flow_cooldown_seconds(value: &Value) -> Option<i64> {
    [
        "cooldown_seconds",
        "reset_after_seconds",
        "retry_after_seconds",
    ]
    .iter()
    .find_map(|key| value.get(*key).and_then(Value::as_i64))
    .filter(|seconds| *seconds > 0)
}

async fn run_check_account_step(
    state: &ServerState,
    manifest: &PluginManifest,
    account: &AccountRecord,
    step: &Value,
    context: &Value,
    host: &ServerPluginHost,
) -> Result<Value, String> {
    let action = step
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if action != "plugin.run_js" {
        return Err(format!(
            "check_account 只支持 plugin.run_js，收到: {action}"
        ));
    }
    let script = read_plugin_scripts(state, &manifest.platform_id, &collect_script_files(step))?;
    run_js_step(script, context.clone(), host.clone())
        .await
        .map_err(|error| format!("{} 账号检查失败: {error}", account.platform))
}

fn login_check_request(
    manifest: &PluginManifest,
    account: &AccountRecord,
) -> BrowserSessionCreateRequest {
    let start_url = if manifest.workspace_url.trim().is_empty() {
        manifest.login_url.clone()
    } else {
        manifest.workspace_url.clone()
    };
    BrowserSessionCreateRequest {
        purpose: BrowserSessionPurpose::DebugPreview,
        platform: account.platform.clone(),
        start_url,
        account_id: Some(account.id.clone()),
        profile_dir: Some(account.profile_dir.clone()),
        headless: Some(true),
        width: Some(1365),
        height: Some(768),
        metadata: browser_metadata_from_account(account),
        ..BrowserSessionCreateRequest::default()
    }
}

fn read_plugin_scripts(
    state: &ServerState,
    platform: &str,
    script_files: &[String],
) -> Result<String, String> {
    if script_files.is_empty() {
        return Err(format!("{platform} check_account 没有配置脚本文件"));
    }
    let plugin_dir = state
        .plugin_loader
        .get_plugin_dir(platform)
        .ok_or_else(|| format!("插件目录未找到: {platform}"))?;
    let mut combined = String::new();
    for script_file in script_files {
        if !safe_script_file(script_file) {
            return Err(format!("账号检查脚本路径不安全: {script_file}"));
        }
        append_script(&mut combined, &plugin_dir.join("scripts").join(script_file))?;
    }
    Ok(combined)
}

fn append_script(combined: &mut String, path: &Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("读取账号检查脚本失败 {}: {error}", path.display()))?;
    combined.push_str(&content);
    combined.push_str("\n;\n");
    Ok(())
}

fn save_flow_result(step: &Value, context: &mut Value, result: &Value) {
    if let Some(key) = step.get("save_as").and_then(Value::as_str) {
        context[key] = result.clone();
    }
}

fn safe_script_file(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_expired_and_inconclusive_results_are_distinct() {
        assert!(matches!(
            classify_flow_value(&json!({
                "status": "login_expired",
                "reason": "HTTP 401"
            })),
            Some(AccountLoginCheckOutcome::Expired(reason)) if reason == "HTTP 401"
        ));
        assert!(matches!(
            classify_flow_value(&json!({
                "status": "inconclusive",
                "reason": "HTTP 403"
            })),
            Some(AccountLoginCheckOutcome::Inconclusive(reason)) if reason == "HTTP 403"
        ));
    }

    #[test]
    fn legacy_false_is_inconclusive_but_logged_in_profile_is_available() {
        assert!(matches!(
            classify_browser_login_value(&json!({ "loggedIn": false })),
            AccountLoginCheckOutcome::Inconclusive(_)
        ));
        assert!(matches!(
            classify_browser_login_value(&json!({
                "loggedIn": true,
                "email": "user@example.com"
            })),
            AccountLoginCheckOutcome::Available(profile)
                if profile.email == "user@example.com"
        ));
    }

    #[test]
    fn flow_uses_nested_health_and_explicit_logged_in_state() {
        assert!(matches!(
            classify_flow_results(&[
                json!({ "health": { "status": "inconclusive", "reason": "HTTP 429" } }),
                json!({ "ignored": true }),
            ]),
            AccountLoginCheckOutcome::Inconclusive(reason) if reason == "HTTP 429"
        ));
        assert!(matches!(
            classify_flow_value(&json!({ "logged_in": false, "reason": "session missing" })),
            Some(AccountLoginCheckOutcome::Expired(reason)) if reason == "session missing"
        ));
    }

    #[test]
    fn quota_check_keeps_login_profile_and_reset_seconds() {
        assert!(matches!(
            classify_flow_value(&json!({
                "logged_in": true,
                "status": "daily_quota_exhausted",
                "reason": "limit resets in 4 hours and 35 minutes",
                "cooldown_seconds": 16_500,
                "account": { "email": "pro@example.test" }
            })),
            Some(AccountLoginCheckOutcome::Unavailable {
                login_state,
                profile,
                cooldown_seconds
            }) if login_state == "daily_quota_exhausted"
                && profile.email == "pro@example.test"
                && cooldown_seconds == Some(16_500)
        ));
    }

    #[test]
    fn only_explicit_login_error_from_flow_is_expired() {
        assert!(matches!(
            classify_flow_error("browser timeout"),
            AccountLoginCheckOutcome::Inconclusive(_)
        ));
        assert!(matches!(
            classify_flow_error("LOGIN_EXPIRED: session invalid"),
            AccountLoginCheckOutcome::Expired(reason) if reason == "session invalid"
        ));
    }
}
