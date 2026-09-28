use mediagen_browser_gateway::BrowserSession;
use mediagen_core::PluginManifest;
use mediagen_plugin_runtime::run_js_step;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

use crate::oauth_login;
use crate::server_plugin_host::ServerPluginHost;
use crate::ServerState;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountLoginProfile {
    #[serde(default)]
    pub quota_windows: Option<Vec<Value>>,
    #[serde(default)]
    pub quota_task_kind: Option<String>,
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
pub(crate) enum LoginDetectionConfig {
    JsScript {
        #[serde(default)]
        script_file: String,
        #[serde(default)]
        script_files: Vec<String>,
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
            script_files,
            await_promise,
        } => {
            let references = configured_script_files(script_file, script_files)?;
            run_login_script_with_retry(state, session, &references, await_promise).await?
        }
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
    let script = read_plugin_scripts(state, &session.platform, &[script_file.to_string()])?;
    let context = plugin_login_context(state, session, script_file)?;
    let host = ServerPluginHost::new_with_metadata(
        proxy_url_from_metadata(&session.metadata),
        &session.metadata,
    )
    .with_storage(
        state.credential_cipher.clone(),
        &session.profile_dir,
        state
            .plugin_loader
            .get_plugin(&session.platform)
            .ok_or("插件不存在")?,
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
    let quota_windows = value
        .get("quota_windows")
        .and_then(Value::as_array)
        .cloned();
    let quota_scope = value["quota_task_kind"].as_str().map(ToOwned::to_owned);
    let value = if let Some(account) = value.get("account") {
        account.clone()
    } else {
        value.clone()
    };
    let mut profile = serde_json::from_value::<AccountLoginProfile>(value)
        .unwrap_or_else(|_| AccountLoginProfile::default());
    profile.quota_task_kind = quota_scope.or(profile.quota_task_kind);
    profile.quota_windows = quota_windows.or(profile.quota_windows);
    if !profile.username.trim().is_empty()
        || !profile.email.trim().is_empty()
        || !profile.user_id.trim().is_empty()
    {
        profile.logged_in = true;
    }
    profile
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
    script_files: &[String],
    await_promise: bool,
) -> Result<AccountLoginProfile, String> {
    let result = run_login_script_value(state, session, script_files, await_promise).await?;
    Ok(profile_from_value(&result))
}

pub(crate) async fn run_login_script_value(
    state: &ServerState,
    session: &BrowserSession,
    script_files: &[String],
    await_promise: bool,
) -> Result<Value, String> {
    let mut script = read_plugin_scripts(state, &session.platform, script_files)?;
    if let Some(manifest) = state.plugin_loader.get_plugin(&session.platform) {
        if let Some(name) = manifest
            .extra
            .get("login_detection")
            .and_then(|v| v["context_file"].as_str())
        {
            if name.is_empty() || name.contains('/') || name.contains('\\') || name == ".." {
                return Err("登录上下文文件名无效".into());
            }
            let path = std::path::Path::new(&session.profile_dir).join(name);
            let value = if path.exists() {
                use mediagen_plugin_runtime::PluginHost;
                let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default())
                    .with_storage(
                        state.credential_cipher.clone(),
                        &session.profile_dir,
                        manifest,
                    );
                let result = host.call("file.read_json", &json!({"path":path})).await;
                if !result.success {
                    return Err("登录上下文读取失败".into());
                }
                result.data
            } else {
                Value::Null
            };
            script = format!("globalThis.__pluginLoginContext = {value};\n{script}");
        }
    }
    state
        .browser_gateway
        .evaluate_script(&session.id, &script, await_promise)
        .await
        .map_err(|err| err.to_string())
}

async fn run_login_script_with_retry(
    state: &ServerState,
    session: &BrowserSession,
    script_files: &[String],
    await_promise: bool,
) -> Result<AccountLoginProfile, String> {
    let mut last = AccountLoginProfile::default();
    for _ in 0..6 {
        match run_login_script(state, session, script_files, await_promise).await {
            Ok(profile) if profile.logged_in => return Ok(profile),
            Ok(profile) => last = profile,
            Err(_) => {}
        }
        sleep(Duration::from_millis(800)).await;
    }
    Ok(last)
}

fn read_plugin_scripts(
    state: &ServerState,
    platform: &str,
    script_files: &[String],
) -> Result<String, String> {
    let plugin_dir = state
        .plugin_loader
        .get_plugin_dir(platform)
        .ok_or_else(|| format!("插件目录未找到: {platform}"))?;
    crate::plugin_scripts::read_plugin_scripts(&plugin_dir, script_files)
}

pub(crate) fn login_detection_config(
    manifest: &PluginManifest,
) -> Result<LoginDetectionConfig, String> {
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

pub(crate) fn configured_script_files(
    script_file: String,
    script_files: Vec<String>,
) -> Result<Vec<String>, String> {
    let references = if script_files.is_empty() {
        vec![script_file]
    } else {
        script_files
    };
    if references.iter().any(|value| value.trim().is_empty()) {
        return Err("登录检测没有配置有效脚本".to_string());
    }
    Ok(references)
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

pub(crate) fn requires_login_verification(state: &ServerState, session: &BrowserSession) -> bool {
    use mediagen_browser_gateway::BrowserSessionPurpose;
    matches!(
        session.purpose,
        BrowserSessionPurpose::AddAccount | BrowserSessionPurpose::ReloginAccount
    ) || (session.purpose == BrowserSessionPurpose::HumanVerification
        && state
            .plugin_loader
            .get_plugin(&session.platform)
            .is_some_and(|manifest| manifest.extra.contains_key("login_credential_capture")))
}
