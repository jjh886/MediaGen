use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::orchestrator::Orchestrator;
use crate::sync_ext::MutexExt;
use crate::types::{LoginState, PlatformAccount, StepResult, TierDef};
use crate::AppState;

use super::account::check_duplicate_account_excluding;

pub(super) async fn add_account_with_plugin_flow(
    state: &Arc<AppState>,
    platform: &str,
    profile_id: &str,
    profiles_dir: &Path,
    profile_dir: PathBuf,
    chrome_path: Option<String>,
    tier_def: Option<TierDef>,
    default_tier: String,
) -> Result<PlatformAccount, String> {
    cleanup_stale_plugin_authorizations(state, platform);

    let account_id = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(&profile_dir)
        .map_err(|e| format!("创建账号目录失败 {}: {e}", profile_dir.to_string_lossy()))?;

    let account = build_plugin_flow_account(
        platform,
        &account_id,
        profile_id,
        profiles_dir,
        tier_def,
        default_tier,
    );

    let flow_result =
        run_plugin_account_flow(state, platform, "add_account", None, &account, chrome_path).await;

    let results = match flow_result {
        Ok(results) => results,
        Err(e) => {
            tracing::error!(
                platform,
                account_id = %account.id,
                profile_dir = %account.profile_dir,
                error = %e,
                "插件账号添加登录失败"
            );
            state
                .browser_engine
                .force_release_profile(&account.profile_dir);
            cleanup_failed_plugin_profile(&account.profile_dir);
            return Err(e);
        }
    };

    finalize_new_plugin_flow_account(state, account, results).map_err(|e| {
        tracing::error!(platform, profile_dir = %profile_dir.display(), error = %e, "插件账号保存失败");
        cleanup_failed_plugin_profile(&profile_dir.to_string_lossy());
        e
    })
}

pub(super) async fn relogin_account_with_plugin_flow(
    state: &Arc<AppState>,
    account: PlatformAccount,
    chrome_path: Option<String>,
) -> Result<PlatformAccount, String> {
    if account.profile_dir.trim().is_empty() {
        return Err("账号 profile_dir 为空，请删除账号重新添加".to_string());
    }
    let profile_dir = PathBuf::from(&account.profile_dir);
    std::fs::create_dir_all(&profile_dir)
        .map_err(|e| format!("创建账号目录失败 {}: {e}", profile_dir.to_string_lossy()))?;

    let results = match run_plugin_account_flow(
        state,
        &account.platform,
        "relogin_account",
        Some("add_account"),
        &account,
        chrome_path,
    )
    .await
    {
        Ok(results) => results,
        Err(e) => {
            tracing::error!(
                platform = %account.platform,
                account_id = %account.id,
                profile_dir = %account.profile_dir,
                error = %e,
                "插件账号重新登录失败"
            );
            state
                .browser_engine
                .force_release_profile(&account.profile_dir);
            return Err(e);
        }
    };

    finalize_plugin_flow_account(state, account.id, results, false)
}

fn build_plugin_flow_account(
    platform: &str,
    account_id: &str,
    profile_id: &str,
    profiles_dir: &Path,
    tier_def: Option<TierDef>,
    default_tier: String,
) -> PlatformAccount {
    let now = chrono::Utc::now();
    let td = tier_def.unwrap_or(TierDef {
        display_name: "免费用户".to_string(),
        is_default: true,
        concurrency_limit: 1,
        daily_limit: 0,
        total_limit: 0,
        cooldown_seconds: 60,
    });
    let profile_dir = profiles_dir
        .join(format!("{}_{}", platform, profile_id))
        .to_string_lossy()
        .to_string();
    PlatformAccount {
        id: account_id.to_string(),
        platform: platform.to_string(),
        name: format!("{}_{}", platform, &profile_id[..6]),
        notes: String::new(),
        login_state: LoginState::LoginExpired,
        login_state_reason: "插件授权中".to_string(),
        login_state_updated_at: now,
        cookies: String::new(),
        api_token: String::new(),
        refresh_token: String::new(),
        token_expires_at: None,
        profile_dir,
        tier: default_tier,
        concurrency_limit: td.concurrency_limit,
        daily_limit: td.daily_limit,
        total_limit: td.total_limit,
        used_today: 0,
        used_total: 0,
        usage_date: now.format("%Y-%m-%d").to_string(),
        cooldown_until: None,
        user_id: String::new(),
        username: String::new(),
        email: String::new(),
        avatar_url: String::new(),
        password_encrypted: String::new(),
        created_at: now,
        last_check_at: None,
        last_refreshed_at: None,
        last_used_at: None,
    }
}

async fn run_plugin_account_flow(
    state: &Arc<AppState>,
    platform: &str,
    flow_name: &str,
    fallback_flow_name: Option<&str>,
    account: &PlatformAccount,
    chrome_path: Option<String>,
) -> Result<Vec<StepResult>, String> {
    let plugin = state
        .plugin_loader
        .get_plugin(platform)
        .ok_or_else(|| format!("平台插件 '{}' 未找到", platform))?;
    let flow = plugin
        .flows
        .get(flow_name)
        .or_else(|| fallback_flow_name.and_then(|name| plugin.flows.get(name)))
        .cloned()
        .ok_or_else(|| format!("插件 '{}' 未配置账号授权 flow: {}", platform, flow_name))?;
    let scripts_dir = state
        .plugin_loader
        .get_plugin_dir(platform)
        .map(|d| d.join("scripts").to_string_lossy().to_string())
        .unwrap_or_default();
    let home_dir = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let initial_context = serde_json::json!({
        "account_id": account.id,
        "platform": platform,
        "profile_dir": account.profile_dir,
        "scripts_dir": scripts_dir,
        "app_data_dir": state.app_data_dir.to_string_lossy(),
        "home_dir": home_dir,
        "chrome_path": chrome_path.unwrap_or_default(),
        "account": account,
    });

    Orchestrator::execute_flow(&flow, &state.engine_registry, initial_context)
        .await
        .map_err(|e| e.to_string())
}

fn cleanup_failed_plugin_account(state: &Arc<AppState>, account_id: &str, profile_dir: &str) {
    {
        let storage = state.storage.lock_recover();
        if let Err(e) = storage.delete_account(account_id) {
            tracing::warn!(account_id, error = %e, "清理失败的插件账号记录失败");
        }
    }
    if !profile_dir.is_empty() {
        cleanup_failed_plugin_profile(profile_dir);
    }
}

fn cleanup_failed_plugin_profile(profile_dir: &str) {
    if profile_dir.is_empty() {
        return;
    }
    let path = Path::new(profile_dir);
    if path.exists() {
        if let Err(e) = std::fs::remove_dir_all(path) {
            tracing::warn!(profile_dir, error = %e, "清理失败的插件账号目录失败");
        }
    }
}

fn cleanup_stale_plugin_authorizations(state: &Arc<AppState>, platform: &str) {
    let stale_accounts = {
        let storage = state.storage.lock_recover();
        match storage.list_accounts_by_platform(platform) {
            Ok(accounts) => accounts
                .into_iter()
                .filter(is_stale_plugin_authorization)
                .collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!(platform, error = %e, "读取插件账号列表失败，跳过残留授权清理");
                Vec::new()
            }
        }
    };

    if stale_accounts.is_empty() {
        return;
    }

    let storage = state.storage.lock_recover();
    for account in stale_accounts {
        if let Err(e) = storage.delete_account(&account.id) {
            tracing::warn!(account_id = %account.id, error = %e, "清理残留插件授权账号失败");
            continue;
        }
        cleanup_failed_plugin_profile(&account.profile_dir);
    }
}

fn is_stale_plugin_authorization(account: &PlatformAccount) -> bool {
    account.login_state == LoginState::LoginExpired
        && account.login_state_reason == "插件授权中"
        && account.cookies.trim().is_empty()
        && account.api_token.trim().is_empty()
        && account.refresh_token.trim().is_empty()
        && account.user_id.trim().is_empty()
        && account.username.trim().is_empty()
        && account.email.trim().is_empty()
}

fn finalize_new_plugin_flow_account(
    state: &Arc<AppState>,
    mut account: PlatformAccount,
    results: Vec<StepResult>,
) -> Result<PlatformAccount, String> {
    apply_successful_plugin_account_update(&mut account, &results);

    let storage = state.storage.lock_recover();
    check_duplicate_account_excluding(&storage, &account, None)?;
    storage
        .insert_account(&account)
        .map_err(|e| e.to_string())?;
    Ok(account)
}

fn finalize_plugin_flow_account(
    state: &Arc<AppState>,
    account_id: String,
    results: Vec<StepResult>,
    cleanup_on_duplicate: bool,
) -> Result<PlatformAccount, String> {
    let storage = state.storage.lock_recover();
    let mut account = storage
        .get_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("账号不存在: {account_id}"))?;
    apply_successful_plugin_account_update(&mut account, &results);
    if let Err(e) = check_duplicate_account_excluding(&storage, &account, Some(&account.id)) {
        drop(storage);
        if cleanup_on_duplicate {
            cleanup_failed_plugin_account(state, &account.id, &account.profile_dir);
        }
        return Err(e);
    }
    storage
        .update_account(&account)
        .map_err(|e| e.to_string())?;
    Ok(account)
}

fn apply_successful_plugin_account_update(account: &mut PlatformAccount, results: &[StepResult]) {
    apply_plugin_account_update(account, extract_plugin_account_update(results));
    account.login_state = LoginState::Available;
    account.login_state_reason = "插件授权成功".to_string();
    account.login_state_updated_at = chrono::Utc::now();
    account.last_check_at = Some(chrono::Utc::now());
    if account.name.trim().is_empty() {
        account.name = if account.username.trim().is_empty() {
            format!("{}_{}", account.platform, &account.id[..6])
        } else {
            account.username.clone()
        };
    }
}

#[derive(Default)]
struct PluginAccountUpdate {
    cookies: Option<String>,
    api_token: Option<String>,
    refresh_token: Option<String>,
    user_id: Option<String>,
    username: Option<String>,
    email: Option<String>,
    avatar_url: Option<String>,
    tier: Option<String>,
    name: Option<String>,
}

fn extract_plugin_account_update(results: &[StepResult]) -> PluginAccountUpdate {
    let mut update = PluginAccountUpdate::default();
    for data in results.iter().map(|r| &r.data) {
        merge_plugin_account_value(&mut update, data);
        if let Some(account) = data.get("account") {
            merge_plugin_account_value(&mut update, account);
        }
        if let Some(credentials) = data.get("credentials") {
            merge_plugin_account_value(&mut update, credentials);
        }
    }
    update
}

fn merge_plugin_account_value(update: &mut PluginAccountUpdate, value: &serde_json::Value) {
    if !value.is_object() {
        return;
    }
    merge_string_field(&mut update.cookies, value, "cookies");
    merge_string_field(&mut update.api_token, value, "api_token");
    merge_string_field(&mut update.refresh_token, value, "refresh_token");
    merge_string_field(&mut update.user_id, value, "user_id");
    merge_string_field(&mut update.username, value, "username");
    merge_string_field(&mut update.email, value, "email");
    merge_string_field(&mut update.avatar_url, value, "avatar_url");
    merge_string_field(&mut update.tier, value, "tier");
    merge_string_field(&mut update.name, value, "name");
}

fn merge_string_field(target: &mut Option<String>, value: &serde_json::Value, key: &str) {
    if let Some(v) = value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        *target = Some(v.to_string());
    }
}

fn apply_plugin_account_update(account: &mut PlatformAccount, update: PluginAccountUpdate) {
    if let Some(v) = update.cookies {
        account.cookies = v;
    }
    if let Some(v) = update.api_token {
        account.api_token = v;
    }
    if let Some(v) = update.refresh_token {
        account.refresh_token = v;
    }
    if let Some(v) = update.user_id {
        account.user_id = v;
    }
    if let Some(v) = update.username {
        account.username = v.clone();
        account.name = v;
    }
    if let Some(v) = update.email {
        account.email = v;
    }
    if let Some(v) = update.avatar_url {
        account.avatar_url = v;
    }
    if let Some(v) = update.tier {
        account.tier = v;
    }
    if let Some(v) = update.name {
        account.name = v;
    }
}
