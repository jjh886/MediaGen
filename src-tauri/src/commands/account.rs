use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;

use crate::browser_manager;
use crate::login_checker;
use crate::plugin_loader::PluginLoader;
use crate::storage::Storage;
use crate::sync_ext::MutexExt;
use crate::types::*;
use crate::AppState;

use super::account_plugin_flow::{add_account_with_plugin_flow, relogin_account_with_plugin_flow};

struct AddAccountConfig {
    use_plugin_account_flow: bool,
    default_tier: String,
    tier_def: Option<TierDef>,
    chrome_path: Option<String>,
}

/// 读取平台的 browser_lifecycle；平台不存在时按默认 per_task 处理。
fn resolve_browser_lifecycle(plugin_loader: &PluginLoader, platform: &str) -> BrowserLifecycle {
    plugin_loader
        .get_plugin(platform)
        .map(|m| m.browser_lifecycle)
        .unwrap_or_default()
}

/// 从 settings 读取 chrome_path，清理引号和空白；未配置时自动查找系统安装路径
pub(crate) fn resolve_chrome_path(storage: &Storage) -> Option<String> {
    let user_path = storage
        .get_setting("chrome_path")
        .ok()?
        .map(|s| s.trim_matches('"').to_string())
        .filter(|s| !s.trim().is_empty());

    if user_path.is_some() {
        return user_path;
    }

    // 用户未配置，遍历系统常见安装路径
    find_system_chrome()
}

/// 返回当前平台的 Chrome 候选路径列表
pub(crate) fn default_chrome_candidates() -> Vec<String> {
    let mut candidates = Vec::new();

    #[cfg(target_os = "macos")]
    {
        candidates.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        candidates.push(r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string());
        candidates.push(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".to_string());
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            candidates.push(format!(
                r"{}\Google\Chrome\Application\chrome.exe",
                local_app_data
            ));
        }
    }

    #[cfg(target_os = "linux")]
    {
        candidates.push("/usr/bin/google-chrome".to_string());
        candidates.push("/usr/bin/google-chrome-stable".to_string());
        candidates.push("/usr/bin/chromium-browser".to_string());
    }

    candidates
}

/// 在系统常见路径中查找第一个存在的 Chrome
pub(crate) fn find_system_chrome() -> Option<String> {
    default_chrome_candidates()
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

/// 从插件配置构建 BrowserLoginConfig（读取 manifest 配置 + 加载脚本文件内容）
pub(crate) fn build_login_config(
    plugin_loader: &PluginLoader,
    platform: &str,
    chrome_path: Option<String>,
    profile_dir: PathBuf,
) -> Result<browser_manager::BrowserLoginConfig, String> {
    let plugin = plugin_loader
        .get_plugin(platform)
        .ok_or_else(|| format!("平台插件 '{}' 未找到", platform))?;
    let plugin_dir = plugin_loader
        .get_plugin_dir(platform)
        .ok_or_else(|| format!("插件目录未找到: {}", platform))?;

    // 读取登录检测配置
    let mut detection = plugin
        .login_detection
        .clone()
        .ok_or_else(|| format!("插件 '{}' 未配置 login_detection", platform))?;

    // 如果是 JS 脚本检测，从插件 scripts/ 目录读取脚本文件内容
    if let LoginDetection::JsScript {
        ref script_file,
        script_content,
        ..
    } = &mut detection
    {
        let script_path = plugin_dir.join("scripts").join(script_file.as_str());
        let content = std::fs::read_to_string(&script_path)
            .map_err(|e| format!("读取脚本 {} 失败: {e}", script_path.display()))?;
        *script_content = Some(content);
    }

    // 读取 userinfo 脚本（可选）
    let (userinfo_script, userinfo_await) = match &plugin.userinfo_script {
        Some(script_ref) => {
            let script_path = plugin_dir.join("scripts").join(&script_ref.script_file);
            let content = std::fs::read_to_string(&script_path)
                .map_err(|e| format!("读取脚本 {} 失败: {e}", script_path.display()))?;
            (Some(content), script_ref.await_promise)
        }
        None => (None, false),
    };

    Ok(browser_manager::BrowserLoginConfig {
        chrome_path,
        profile_dir,
        login_url: plugin.login_url.clone(),
        detection,
        userinfo_script,
        userinfo_await_promise: userinfo_await,
    })
}

/// 添加新账号：创建新 profile，打开 Chrome 等待用户登录
#[tauri::command]
pub async fn add_account_start(
    state: tauri::State<'_, Arc<AppState>>,
    app_handle: tauri::AppHandle,
    platform: String,
) -> Result<PlatformAccount, String> {
    let app_state = state.inner().clone();
    let add_config = prepare_add_account_config(&state, &platform)?;
    let (profile_id, profiles_dir, profile_dir) = new_profile_paths(&app_handle, &platform)?;

    if add_config.use_plugin_account_flow {
        return add_account_with_plugin_flow(
            &app_state,
            &platform,
            &profile_id,
            &profiles_dir,
            profile_dir,
            add_config.chrome_path,
            add_config.tier_def,
            add_config.default_tier,
        )
        .await;
    }

    let login_result =
        add_account_with_browser(&state, &platform, add_config.chrome_path, profile_dir).await?;
    let account = build_new_account(
        &platform,
        &profile_id,
        &profiles_dir,
        add_config.tier_def,
        add_config.default_tier,
        login_result,
    );
    insert_new_account(&state, &account)?;
    Ok(account)
}

fn prepare_add_account_config(
    state: &AppState,
    platform: &str,
) -> Result<AddAccountConfig, String> {
    let plugin = state
        .plugin_loader
        .get_plugin(platform)
        .ok_or_else(|| format!("平台插件 '{}' 未找到", platform))?;
    let default_tier = plugin
        .tiers
        .iter()
        .find(|(_, t)| t.is_default)
        .map(|(k, _)| k.clone())
        .unwrap_or_else(|| "free".to_string());
    let tier_def = plugin.tiers.get(&default_tier).cloned();
    let storage = state.storage.lock_recover();
    Ok(AddAccountConfig {
        use_plugin_account_flow: matches!(plugin.adapter_mode, AdapterMode::Api)
            && plugin.flows.contains_key("add_account"),
        chrome_path: resolve_chrome_path(&storage),
        tier_def: tier_def
            .map(|td| crate::tier_override::apply_overrides(&storage, platform, &default_tier, td)),
        default_tier,
    })
}

fn new_profile_paths(
    app_handle: &tauri::AppHandle,
    platform: &str,
) -> Result<(String, PathBuf, PathBuf), String> {
    let profiles_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("profiles");
    let profile_id = uuid::Uuid::new_v4().to_string();
    let profile_dir = profiles_dir.join(format!("{}_{}", platform, profile_id));
    Ok((profile_id, profiles_dir, profile_dir))
}

async fn add_account_with_browser(
    state: &AppState,
    platform: &str,
    chrome_path: Option<String>,
    profile_dir: PathBuf,
) -> Result<crate::browser_manager::LoginResult, String> {
    let config = build_login_config(&state.plugin_loader, platform, chrome_path, profile_dir)?;
    let lifecycle = resolve_browser_lifecycle(&state.plugin_loader, platform);
    let engine = state.browser_engine.clone();
    tokio::task::spawn_blocking(move || {
        browser_manager::open_and_wait_for_login(config, lifecycle, Some(engine))
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))?
}

fn insert_new_account(state: &AppState, account: &PlatformAccount) -> Result<(), String> {
    let storage = state.storage.lock_recover();
    check_duplicate_account(&storage, account)?;
    storage.insert_account(account).map_err(|e| e.to_string())
}

fn default_account_tier_def() -> TierDef {
    TierDef {
        display_name: "免费用户".to_string(),
        is_default: true,
        concurrency_limit: 1,
        daily_limit: 0,
        total_limit: 0,
        cooldown_seconds: 60,
    }
}

fn build_new_account(
    platform: &str,
    profile_id: &str,
    profiles_dir: &std::path::Path,
    tier_def: Option<TierDef>,
    default_tier: String,
    login_result: crate::browser_manager::LoginResult,
) -> PlatformAccount {
    let now = chrono::Utc::now();
    let td = tier_def.unwrap_or_else(default_account_tier_def);
    let name = if login_result.username.is_empty() {
        format!("{}_{}", platform, &profile_id[..6])
    } else {
        login_result.username.clone()
    };
    let tier = if login_result.tier.is_empty() {
        default_tier
    } else {
        login_result.tier
    };
    PlatformAccount {
        id: uuid::Uuid::new_v4().to_string(),
        platform: platform.to_string(),
        name,
        notes: String::new(),
        login_state: LoginState::Available,
        login_state_reason: "首次登录".to_string(),
        login_state_updated_at: now,
        cookies: login_result.cookies_json,
        api_token: String::new(),
        refresh_token: String::new(),
        token_expires_at: None,
        profile_dir: profiles_dir
            .join(format!("{}_{}", platform, profile_id))
            .to_string_lossy()
            .to_string(),
        tier,
        concurrency_limit: td.concurrency_limit,
        daily_limit: td.daily_limit,
        total_limit: td.total_limit,
        used_today: 0,
        used_total: 0,
        usage_date: now.format("%Y-%m-%d").to_string(),
        cooldown_until: None,
        user_id: login_result.user_id,
        username: login_result.username,
        email: login_result.email,
        avatar_url: login_result.avatar_url,
        password_encrypted: String::new(),
        created_at: now,
        last_check_at: Some(now),
        last_refreshed_at: None,
        last_used_at: None,
    }
}

fn check_duplicate_account(storage: &Storage, account: &PlatformAccount) -> Result<(), String> {
    check_duplicate_account_excluding(storage, account, None)
}

pub(super) fn check_duplicate_account_excluding(
    storage: &Storage,
    account: &PlatformAccount,
    exclude_id: Option<&str>,
) -> Result<(), String> {
    if account.user_id.is_empty() {
        return Ok(());
    }
    let existing = storage
        .list_accounts_by_platform(&account.platform)
        .map_err(|e| e.to_string())?;
    if existing.iter().any(|a| {
        !a.user_id.is_empty()
            && a.user_id == account.user_id
            && exclude_id.map(|id| a.id != id).unwrap_or(true)
    }) {
        return Err(format!(
            "该 {} 账号已存在（user_id: {}）",
            account.platform, account.user_id
        ));
    }
    Ok(())
}

/// 重新登录：复用已有 profile_dir，打开 Chrome 等待用户登录
#[tauri::command]
pub async fn relogin_account(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<PlatformAccount, String> {
    let app_state = state.inner().clone();
    let account = load_account_for_relogin(&state, &id)?;
    if uses_plugin_account_flow(&state, &account.platform) {
        let chrome_path = {
            let storage = state.storage.lock_recover();
            resolve_chrome_path(&storage)
        };
        return relogin_account_with_plugin_flow(&app_state, account, chrome_path).await;
    }

    let login_result = relogin_with_browser(&state, &account).await?;
    update_relogin_account(&state, &id, login_result)
}

fn load_account_for_relogin(state: &AppState, id: &str) -> Result<PlatformAccount, String> {
    let storage = state.storage.lock_recover();
    storage
        .get_account(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "账号不存在".to_string())
}

fn uses_plugin_account_flow(state: &AppState, platform: &str) -> bool {
    state
        .plugin_loader
        .get_plugin(platform)
        .map(|p| matches!(p.adapter_mode, AdapterMode::Api) && p.flows.contains_key("add_account"))
        .unwrap_or(false)
}

async fn relogin_with_browser(
    state: &AppState,
    account: &PlatformAccount,
) -> Result<crate::browser_manager::LoginResult, String> {
    let chrome_path = {
        let storage = state.storage.lock_recover();
        resolve_chrome_path(&storage)
    };
    let profile_dir = PathBuf::from(&account.profile_dir);
    if !profile_dir.exists() {
        return Err("Profile 目录不存在，请删除账号重新添加".to_string());
    }

    let config = build_login_config(
        &state.plugin_loader,
        &account.platform,
        chrome_path,
        profile_dir,
    )?;
    let lifecycle = resolve_browser_lifecycle(&state.plugin_loader, &account.platform);
    // Persistent 平台：重新登录前先释放旧的常驻浏览器，避免 Chrome 起不来或复用到旧 session
    if matches!(lifecycle, BrowserLifecycle::Persistent) {
        state
            .browser_engine
            .force_release_profile(&account.profile_dir);
    }
    let engine = state.browser_engine.clone();
    tokio::task::spawn_blocking(move || {
        browser_manager::open_and_wait_for_login(config, lifecycle, Some(engine))
    })
    .await
    .map_err(|e| format!("任务执行失败: {e}"))?
}

fn update_relogin_account(
    state: &AppState,
    id: &str,
    login_result: crate::browser_manager::LoginResult,
) -> Result<PlatformAccount, String> {
    let storage = state.storage.lock_recover();
    let mut account = storage
        .get_account(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "账号不存在".to_string())?;
    account.cookies = login_result.cookies_json;
    if !login_result.username.is_empty() {
        account.username = login_result.username.clone();
        account.name = login_result.username;
    }
    if !login_result.user_id.is_empty() {
        account.user_id = login_result.user_id;
    }
    if !login_result.email.is_empty() {
        account.email = login_result.email;
    }
    if !login_result.avatar_url.is_empty() {
        account.avatar_url = login_result.avatar_url;
    }
    if !login_result.tier.is_empty() {
        account.tier = login_result.tier;
    }
    account.login_state = LoginState::Available;
    account.login_state_reason = "重新登录成功".to_string();
    account.login_state_updated_at = chrono::Utc::now();
    account.last_check_at = Some(chrono::Utc::now());
    storage
        .update_account(&account)
        .map_err(|e| e.to_string())?;
    Ok(account)
}

/// 手动触发所有账号的登录状态检查
#[tauri::command]
pub async fn check_accounts_login(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let (checked, expired) =
        login_checker::check_all_accounts(&state.storage, &state.plugin_loader).await;
    Ok(serde_json::json!({ "checked": checked, "expired": expired }))
}
