use mediagen_browser_gateway::{
    BrowserSession, BrowserSessionCreateRequest, BrowserSessionPurpose,
};
use mediagen_core::PluginManifest;
use mediagen_plugin_runtime::run_js_step;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use tokio::time::{sleep, Duration};

use crate::account_browser_profile::browser_metadata_from_account;
use crate::flow_templates::collect_script_files;
use crate::oauth_login;
use crate::server_plugin_host::ServerPluginHost;
use crate::store::AccountRecord;
use crate::ServerState;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountLoginProfile {
    #[serde(default, alias = "loggedIn")]
    pub logged_in: bool,
    #[serde(default, alias = "userId")]
    pub user_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default, alias = "avatarUrl")]
    pub avatar_url: String,
    #[serde(default)]
    pub tier: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum LoginDetectionConfig {
    JsScript {
        script_file: String,
        #[serde(default)]
        await_promise: bool,
    },
    PluginScript {
        script_file: String,
    },
    CdpCookies {
        cookie_names: Vec<String>,
    },
}

pub async fn verify_browser_session_login(
    state: &ServerState,
    session: &BrowserSession,
) -> Result<AccountLoginProfile, String> {
    let manifest = state
        .plugin_loader
        .get_plugin(&session.platform)
        .ok_or_else(|| format!("平台插件 '{}' 未找到", session.platform))?;
    let detection = login_detection_config(manifest)?;
    let profile = match detection {
        LoginDetectionConfig::JsScript {
            script_file,
            await_promise,
        } => run_login_script_with_retry(state, session, &script_file, await_promise).await?,
        LoginDetectionConfig::PluginScript { script_file } => {
            run_login_plugin_script(state, session, &script_file).await?
        }
        LoginDetectionConfig::CdpCookies { cookie_names } => {
            return Err(format!(
                "{} 暂不支持服务端 cookie 登录检测，请改为 js_script 检测。缺少: {}",
                manifest.platform_id,
                cookie_names.join(", ")
            ));
        }
    };
    require_logged_in(profile, "还没有检测到登录成功，请继续在页面里完成登录。")
}

async fn run_login_plugin_script(
    state: &ServerState,
    session: &BrowserSession,
    script_file: &str,
) -> Result<AccountLoginProfile, String> {
    let script = read_plugin_script(state, &session.platform, script_file)?;
    let context = plugin_login_context(state, session, script_file)?;
    let host = ServerPluginHost::new_with_metadata(
        proxy_url_from_metadata(&session.metadata),
        &session.metadata,
    );
    let result = run_js_step(script, context, host).await?;
    Ok(profile_from_value(&result))
}

fn plugin_login_context(
    state: &ServerState,
    session: &BrowserSession,
    script_file: &str,
) -> Result<Value, String> {
    let mut context = json!({
        "platform": session.platform,
        "profile_dir": session.profile_dir,
        "account_id": session.account_id,
    });
    let Some(meta) = oauth_login::oauth_meta(&session.metadata) else {
        return Ok(context);
    };
    let callback = state
        .oauth_logins
        .take_callback(&meta.state)
        .ok_or_else(|| "等待 OAuth 授权回调，请在登录页面继续操作。".to_string())?;
    if !callback.error.is_empty() {
        return Err(first_non_empty(&[
            callback.error_description.as_str(),
            callback.error.as_str(),
        ]));
    }
    if callback.code.trim().is_empty() {
        return Err("OAuth 回调缺少授权码，请重新登录。".to_string());
    }
    let configured_script = first_non_empty(&[meta.token_script_file.as_str(), script_file]);
    context["oauth"] = json!({
        "code": callback.code,
        "redirect_uri": meta.redirect_uri,
        "code_verifier": meta.code_verifier,
        "token_url": meta.token_url,
        "client_id": meta.client_id,
        "token_script_file": configured_script,
    });
    Ok(context)
}

pub async fn check_saved_account_login(
    state: &ServerState,
    account: &AccountRecord,
) -> Result<AccountLoginProfile, String> {
    let manifest = state
        .plugin_loader
        .get_plugin(&account.platform)
        .ok_or_else(|| format!("平台插件 '{}' 未找到", account.platform))?;
    if let Some(profile) = check_saved_account_with_flow(state, manifest, account).await? {
        return Ok(profile);
    }
    check_saved_account_with_browser(state, manifest, account).await
}

async fn check_saved_account_with_flow(
    state: &ServerState,
    manifest: &PluginManifest,
    account: &AccountRecord,
) -> Result<Option<AccountLoginProfile>, String> {
    let Some(flow) = manifest.flows.get("check_account") else {
        return Ok(None);
    };
    let host = ServerPluginHost::new_for_account(account);
    let mut context = json!({
        "platform": account.platform,
        "account": account,
        "account_id": account.id,
        "profile_dir": account.profile_dir,
    });
    let mut last = Value::Null;
    for step in &flow.steps {
        let result =
            run_check_account_step(state, manifest, account, step, &context, &host).await?;
        save_flow_result(step, &mut context, &result);
        last = result;
    }
    let profile = profile_result_from_value(&last)?;
    Ok(Some(profile))
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
        .map_err(|err| format!("{} 账号检查失败: {err}", account.platform))
}

async fn check_saved_account_with_browser(
    state: &ServerState,
    manifest: &PluginManifest,
    account: &AccountRecord,
) -> Result<AccountLoginProfile, String> {
    let start_url = if manifest.workspace_url.trim().is_empty() {
        manifest.login_url.clone()
    } else {
        manifest.workspace_url.clone()
    };
    let response = state
        .browser_gateway
        .create_session(BrowserSessionCreateRequest {
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
        })
        .await
        .map_err(|err| err.to_string())?;
    let session_id = response.session.id.clone();
    let result = verify_browser_session_login(state, &response.session).await;
    let _ = state.browser_gateway.cancel_session(&session_id).await;
    result
}

pub fn default_tier(manifest: &PluginManifest) -> String {
    manifest
        .tiers
        .iter()
        .find(|(_, tier)| tier.is_default)
        .map(|(id, _)| id.clone())
        .or_else(|| manifest.tiers.keys().next().cloned())
        .unwrap_or_else(|| "standard".to_string())
}

pub fn profile_from_value(value: &Value) -> AccountLoginProfile {
    let value = if let Some(account) = value.get("account") {
        account.clone()
    } else {
        value.clone()
    };
    let mut profile = serde_json::from_value::<AccountLoginProfile>(value)
        .unwrap_or_else(|_| AccountLoginProfile::default());
    if !profile.username.trim().is_empty()
        || !profile.email.trim().is_empty()
        || !profile.user_id.trim().is_empty()
    {
        profile.logged_in = true;
    }
    profile
}

fn profile_result_from_value(value: &Value) -> Result<AccountLoginProfile, String> {
    let profile = profile_from_value(value);
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("账号登录状态不可用，请重新登录。");
    require_logged_in(profile, reason)
}

fn require_logged_in(
    profile: AccountLoginProfile,
    message: &str,
) -> Result<AccountLoginProfile, String> {
    if profile.logged_in {
        return Ok(profile);
    }
    Err(message.to_string())
}

async fn run_login_script(
    state: &ServerState,
    session: &BrowserSession,
    script_file: &str,
    await_promise: bool,
) -> Result<AccountLoginProfile, String> {
    let script = read_plugin_script(state, &session.platform, script_file)?;
    let result = state
        .browser_gateway
        .evaluate_script(&session.id, &script, await_promise)
        .await
        .map_err(|err| err.to_string())?;
    Ok(profile_from_value(&result))
}

async fn run_login_script_with_retry(
    state: &ServerState,
    session: &BrowserSession,
    script_file: &str,
    await_promise: bool,
) -> Result<AccountLoginProfile, String> {
    let mut last = AccountLoginProfile::default();
    for _ in 0..6 {
        match run_login_script(state, session, script_file, await_promise).await {
            Ok(profile) if profile.logged_in => return Ok(profile),
            Ok(profile) => last = profile,
            Err(_) => {}
        }
        sleep(Duration::from_millis(800)).await;
    }
    Ok(last)
}

fn read_plugin_script(
    state: &ServerState,
    platform: &str,
    script_file: &str,
) -> Result<String, String> {
    if !is_safe_script_file(script_file) {
        return Err(format!("登录检测脚本路径不安全: {script_file}"));
    }
    let plugin_dir = state
        .plugin_loader
        .get_plugin_dir(platform)
        .ok_or_else(|| format!("插件目录未找到: {platform}"))?;
    let script_path = plugin_dir.join("scripts").join(script_file);
    std::fs::read_to_string(&script_path)
        .map_err(|err| format!("读取登录检测脚本失败 {}: {err}", script_path.display()))
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
        if !is_safe_script_file(script_file) {
            return Err(format!("账号检查脚本路径不安全: {script_file}"));
        }
        append_script(&mut combined, &plugin_dir.join("scripts").join(script_file))?;
    }
    Ok(combined)
}

fn append_script(combined: &mut String, script_path: &Path) -> Result<(), String> {
    // 多脚本按 manifest 顺序拼接，保证共享 helper 先于 main 定义。
    let content = std::fs::read_to_string(script_path)
        .map_err(|err| format!("读取账号检查脚本失败 {}: {err}", script_path.display()))?;
    combined.push_str(&content);
    combined.push_str("\n;\n");
    Ok(())
}

fn save_flow_result(step: &Value, context: &mut Value, result: &Value) {
    if let Some(key) = step.get("save_as").and_then(Value::as_str) {
        context[key] = result.clone();
    }
}

fn login_detection_config(manifest: &PluginManifest) -> Result<LoginDetectionConfig, String> {
    let value = manifest
        .extra
        .get("login_detection")
        .or_else(|| manifest.extra.get("login_check"))
        .cloned()
        .unwrap_or(Value::Null);
    if value.is_null() {
        return Err(format!(
            "{} 插件没有配置登录检测脚本。",
            manifest.platform_id
        ));
    }
    serde_json::from_value(value).map_err(|err| format!("登录检测配置解析失败: {err}"))
}

fn is_safe_script_file(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..")
}

fn proxy_url_from_metadata(metadata: &HashMap<String, Value>) -> String {
    metadata
        .get("proxy_server")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}
