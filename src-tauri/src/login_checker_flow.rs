use std::sync::{Arc, Mutex};

use tracing::{info, warn};

use crate::engine::EngineRegistry;
use crate::orchestrator::Orchestrator;
use crate::plugin_loader::PluginLoader;
use crate::storage::Storage;
use crate::sync_ext::MutexExt;
use crate::types::{FlowDef, LoginState, PlatformAccount, PluginManifest, StepResult};

pub(crate) enum AccountCheckOutcome {
    Valid(Option<AccountPatch>),
    Expired(String),
    Inconclusive(String),
}

#[derive(Default)]
pub(crate) struct AccountPatch {
    pub(crate) api_token: Option<String>,
    pub(crate) refresh_token: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) avatar_url: Option<String>,
    pub(crate) tier: Option<String>,
    pub(crate) name: Option<String>,
}

pub(crate) async fn check_account_flow(
    account: &PlatformAccount,
    manifest: &PluginManifest,
    plugin_loader: &Arc<PluginLoader>,
    flow: &FlowDef,
    engine: &EngineRegistry,
) -> AccountCheckOutcome {
    let context = build_account_flow_context(account, manifest, plugin_loader);
    match Orchestrator::execute_flow(flow, engine, context).await {
        Ok(results) => classify_flow_results(&results),
        Err(error) => classify_flow_error(&error.to_string()),
    }
}

fn build_account_flow_context(
    account: &PlatformAccount,
    manifest: &PluginManifest,
    plugin_loader: &Arc<PluginLoader>,
) -> serde_json::Value {
    let scripts_dir = plugin_loader
        .get_plugin_dir(&manifest.platform_id)
        .map(|d| d.join("scripts").to_string_lossy().to_string())
        .unwrap_or_default();
    serde_json::json!({
        "account_id": account.id,
        "platform": manifest.platform_id,
        "profile_dir": account.profile_dir,
        "scripts_dir": scripts_dir,
        "account": account,
    })
}

fn classify_flow_results(results: &[StepResult]) -> AccountCheckOutcome {
    for data in results.iter().rev().map(|result| &result.data) {
        if let Some(outcome) = classify_flow_value(data) {
            return outcome;
        }
        if let Some(health) = data.get("health").and_then(classify_flow_value) {
            return health;
        }
    }
    AccountCheckOutcome::Inconclusive("账号检查脚本没有返回登录状态".to_string())
}

pub(crate) fn classify_flow_value(value: &serde_json::Value) -> Option<AccountCheckOutcome> {
    match value.get("status").and_then(|v| v.as_str()).unwrap_or("") {
        "available" => return Some(AccountCheckOutcome::Valid(extract_account_patch(value))),
        "login_expired" => return Some(AccountCheckOutcome::Expired(flow_reason(value))),
        "inconclusive" => return Some(AccountCheckOutcome::Inconclusive(flow_reason(value))),
        _ => {}
    }
    value
        .get("logged_in")
        .and_then(|v| v.as_bool())
        .map(|logged_in| {
            if logged_in {
                AccountCheckOutcome::Valid(extract_account_patch(value))
            } else {
                AccountCheckOutcome::Expired(flow_reason(value))
            }
        })
}

fn classify_flow_error(message: &str) -> AccountCheckOutcome {
    if let Some(reason) = message.split("LOGIN_EXPIRED:").nth(1) {
        return AccountCheckOutcome::Expired(reason.trim().to_string());
    }
    AccountCheckOutcome::Inconclusive(message.to_string())
}

fn flow_reason(value: &serde_json::Value) -> String {
    value
        .get("reason")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("账号健康检查失败")
        .to_string()
}

fn extract_account_patch(value: &serde_json::Value) -> Option<AccountPatch> {
    let mut patch = AccountPatch::default();
    merge_account_patch(&mut patch, value);
    if let Some(account) = value.get("account") {
        merge_account_patch(&mut patch, account);
    }
    patch_has_value(&patch).then_some(patch)
}

fn merge_account_patch(patch: &mut AccountPatch, value: &serde_json::Value) {
    merge_string_field(&mut patch.api_token, value, "api_token");
    merge_string_field(&mut patch.refresh_token, value, "refresh_token");
    merge_string_field(&mut patch.user_id, value, "user_id");
    merge_string_field(&mut patch.username, value, "username");
    merge_string_field(&mut patch.email, value, "email");
    merge_string_field(&mut patch.avatar_url, value, "avatar_url");
    merge_string_field(&mut patch.tier, value, "tier");
    merge_string_field(&mut patch.name, value, "name");
}

fn merge_string_field(target: &mut Option<String>, value: &serde_json::Value, key: &str) {
    if let Some(text) = value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        *target = Some(text.to_string());
    }
}

fn patch_has_value(patch: &AccountPatch) -> bool {
    patch.api_token.is_some()
        || patch.refresh_token.is_some()
        || patch.user_id.is_some()
        || patch.username.is_some()
        || patch.email.is_some()
        || patch.avatar_url.is_some()
        || patch.tier.is_some()
        || patch.name.is_some()
}

pub(crate) fn persist_account_outcome(
    storage: &Arc<Mutex<Storage>>,
    account: &PlatformAccount,
    outcome: AccountCheckOutcome,
) -> bool {
    match outcome {
        AccountCheckOutcome::Expired(reason) => {
            mark_account_expired(storage, account, &reason);
            true
        }
        AccountCheckOutcome::Valid(patch) => {
            mark_account_valid(storage, account, patch);
            false
        }
        AccountCheckOutcome::Inconclusive(reason) => {
            warn!(account_id = %account.id, platform = %account.platform, reason = %reason, "账号检查结果不确定（不标记过期）");
            mark_account_checked(storage, account);
            false
        }
    }
}

fn mark_account_expired(storage: &Arc<Mutex<Storage>>, account: &PlatformAccount, reason: &str) {
    info!(
        account_id = %account.id,
        platform = %account.platform,
        name = %account.name,
        reason = %reason,
        "账号登录已过期"
    );
    let s = storage.lock_recover();
    if let Err(e) = s.update_login_state(&account.id, LoginState::LoginExpired, reason) {
        warn!(error = %e, "更新登录状态失败");
    }
}

fn mark_account_valid(
    storage: &Arc<Mutex<Storage>>,
    account: &PlatformAccount,
    patch: Option<AccountPatch>,
) {
    match patch {
        Some(patch) => update_account_with_patch(storage, account, patch),
        None => mark_account_available_checked(storage, account),
    }
}

fn mark_account_available_checked(storage: &Arc<Mutex<Storage>>, account: &PlatformAccount) {
    let s = storage.lock_recover();
    if account.login_state != LoginState::Available {
        if let Err(e) = s.update_login_state(&account.id, LoginState::Available, "账号健康检查通过")
        {
            warn!(error = %e, account_id = %account.id, "恢复账号登录状态失败");
        }
    }
    if let Err(e) = s.update_last_check_at(&account.id) {
        warn!(error = %e, account_id = %account.id, "更新 last_check_at 失败");
    }
}

fn update_account_with_patch(
    storage: &Arc<Mutex<Storage>>,
    account: &PlatformAccount,
    patch: AccountPatch,
) {
    let s = storage.lock_recover();
    let Ok(Some(mut current)) = s.get_account(&account.id) else {
        return;
    };
    apply_account_patch(&mut current, patch);
    current.login_state = LoginState::Available;
    current.login_state_reason = "账号健康检查通过".to_string();
    current.login_state_updated_at = chrono::Utc::now();
    current.last_check_at = Some(chrono::Utc::now());
    if let Err(e) = s.update_account(&current) {
        warn!(error = %e, account_id = %account.id, "更新账号健康检查结果失败");
    }
}

fn apply_account_patch(account: &mut PlatformAccount, patch: AccountPatch) {
    if let Some(v) = patch.api_token {
        account.api_token = v;
    }
    if let Some(v) = patch.refresh_token {
        account.refresh_token = v;
    }
    if let Some(v) = patch.user_id {
        account.user_id = v;
    }
    if let Some(v) = patch.username {
        account.username = v.clone();
        account.name = v;
    }
    if let Some(v) = patch.email {
        account.email = v;
    }
    if let Some(v) = patch.avatar_url {
        account.avatar_url = v;
    }
    if let Some(v) = patch.tier {
        account.tier = v;
    }
    if let Some(v) = patch.name {
        account.name = v;
    }
}

fn mark_account_checked(storage: &Arc<Mutex<Storage>>, account: &PlatformAccount) {
    let s = storage.lock_recover();
    if let Err(e) = s.update_last_check_at(&account.id) {
        warn!(error = %e, account_id = %account.id, "更新 last_check_at 失败");
    }
}
