use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::access_control::mask_account_keys;
use crate::net_client::proxy_url_without_auth;
use crate::pagination::{paginate, PageQuery};
use crate::{
    store::{AccountRecord, AccountUpdate, SettingsUpdate},
    ServerState,
};

pub async fn health() -> impl Responder {
    let install_root = std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "service": "mediagen-server",
        "version": env!("CARGO_PKG_VERSION"),
        "install_root": install_root,
        "pid": std::process::id()
    }))
}

pub async fn setup() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "configured": true
    }))
}

pub async fn plugins(state: web::Data<ServerState>) -> impl Responder {
    let data: Vec<_> = state
        .plugin_loader
        .sorted_plugins()
        .into_iter()
        .map(|manifest| {
            serde_json::json!({
                "platform_id": manifest.platform_id,
                "display_name": manifest.display_name,
                "sort_order": manifest.sort_order,
                "adapter_mode": manifest.adapter_mode,
                "login_url": manifest.login_url,
                "workspace_url": manifest.workspace_url,
                "models": manifest.models,
                "tiers": manifest.tiers,
                // 配置结构由插件声明；服务端只按公共协议原样透传，不解释字段含义。
                "generation_settings": manifest.extra.get("generation_settings").cloned().unwrap_or(Value::Null),
                "model_count": manifest.models.len(),
                "supports_registration": manifest.extra
                    .get("registration")
                    .and_then(|value| value.get("enabled"))
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false),
            })
        })
        .collect();
    HttpResponse::Ok().json(serde_json::json!({ "data": data }))
}

#[derive(Deserialize)]
pub struct AccountQuery {
    platform: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

pub async fn accounts(
    state: web::Data<ServerState>,
    query: web::Query<AccountQuery>,
) -> impl Responder {
    let mut accounts = state.store.list_accounts(query.platform.as_deref());
    accounts.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let page = PageQuery::new(query.limit, query.offset);
    let data = paginate(accounts, &page)
        .into_iter()
        .map(public_account)
        .collect::<Vec<_>>();
    HttpResponse::Ok().json(json!({ "data": data }))
}

pub async fn account(state: web::Data<ServerState>, path: web::Path<String>) -> impl Responder {
    match state.store.account_by_id(&path.into_inner()) {
        Some(account) => HttpResponse::Ok().json(json!({ "data": public_account(account) })),
        None => HttpResponse::NotFound().json(json!({ "error": "account not found" })),
    }
}

pub async fn account_summary(state: web::Data<ServerState>) -> impl Responder {
    let accounts = state.store.list_accounts(None);
    let available = accounts
        .iter()
        .filter(|account| account.login_state == "available")
        .count();
    HttpResponse::Ok().json(json!({
        "data": { "available": available, "total": accounts.len() }
    }))
}

pub async fn update_account(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    payload: web::Json<AccountUpdate>,
) -> impl Responder {
    match state
        .store
        .update_account(&path.into_inner(), payload.into_inner())
    {
        Some(account) => HttpResponse::Ok().json(json!({ "data": public_account(account) })),
        None => HttpResponse::NotFound().json(json!({ "error": "account not found" })),
    }
}

pub async fn delete_account(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    if state.store.delete_account(&path.into_inner()) {
        HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
    } else {
        HttpResponse::NotFound().json(serde_json::json!({ "error": "account not found" }))
    }
}

#[derive(Deserialize)]
pub struct ToggleAccountPayload {
    disabled: bool,
}

pub async fn toggle_account(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    payload: web::Json<ToggleAccountPayload>,
) -> impl Responder {
    match state
        .store
        .toggle_account(&path.into_inner(), payload.disabled)
    {
        Some(account) => HttpResponse::Ok().json(json!({ "data": public_account(account) })),
        None => HttpResponse::NotFound().json(json!({ "error": "account not found" })),
    }
}

pub(crate) fn public_account(account: AccountRecord) -> Value {
    let api_key = mask_account_keys(&account.api_key);
    json!({
        "id": account.id,
        "platform": account.platform,
        "name": account.name,
        "email": account.email,
        "avatar_url": account.avatar_url,
        "user_id": account.policy.user_id,
        "priority": account.policy.priority,
        "text_concurrency_limit": account.policy.text_concurrency_limit,
        "text_daily_limit": account.policy.text_daily_limit,
        "text_total_limit": account.policy.text_total_limit,
        "text_used_today": account.policy.text_used_today,
        "text_used_total": account.policy.text_used_total,
        "quota_task_kind": account.policy.quota_task_kind,
        "quota_windows": account.policy.quota_windows,
        "quota_holds": account.policy.quota_holds,
        "login_state_reason": account.policy.login_state_reason,
        "last_check_at": account.policy.last_check_at,
        "last_success_at": account.policy.last_success_at,
        "tier": account.tier,
        "login_state": account.login_state,
        "cooldown_until": account.cooldown_until,
        "used_today": account.used_today,
        "daily_limit": account.daily_limit,
        "used_total": account.used_total,
        "total_limit": account.total_limit,
        "concurrency_limit": account.concurrency_limit,
        "task_interval_seconds": account.task_interval_seconds,
        "proxy_url": proxy_url_without_auth(&account.proxy_url),
        "proxy_id": account.proxy_id,
        "browser_os": account.browser_os,
        "browser_version": account.browser_version,
        "api_key": "",
        "api_key_masked": api_key,
        "api_key_configured": !account.api_key.trim().is_empty(),
        "profile_dir": account.profile_dir,
        "created_at": account.created_at,
    })
}

pub async fn check_accounts_login(state: web::Data<ServerState>) -> impl Responder {
    let summary = crate::account_status_refresh::refresh_accounts(&state).await;
    HttpResponse::Ok().json(summary)
}

pub async fn tasks(
    state: web::Data<ServerState>,
    query: web::Query<AccountQuery>,
) -> impl Responder {
    let page = PageQuery::new(query.limit, query.offset);
    let data = paginate(state.store.list_tasks(query.platform.as_deref()), &page);
    HttpResponse::Ok().json(serde_json::json!({ "data": data }))
}

pub async fn cancel_task(state: web::Data<ServerState>, path: web::Path<String>) -> impl Responder {
    let task_id = path.into_inner();
    match state.store.cancel_task_or_generation_job(&task_id) {
        Ok(task) => {
            state.generation_queue_notify.notify_waiters();
            cancel_task_browser_sessions(&state, &task_id).await;
            HttpResponse::Ok().json(serde_json::json!({ "data": task }))
        }
        Err(error) if error == "task not found" => {
            HttpResponse::NotFound().json(serde_json::json!({ "error": error }))
        }
        Err(error) => HttpResponse::Conflict().json(serde_json::json!({ "error": error })),
    }
}

pub(crate) async fn cancel_task_browser_sessions(state: &ServerState, task_id: &str) {
    let session_ids = state
        .browser_gateway
        .list_sessions()
        .unwrap_or_default()
        .into_iter()
        .filter(|session| {
            session.task_id.as_deref() == Some(task_id)
                && session.status == mediagen_browser_gateway::BrowserSessionStatus::Active
        })
        .map(|session| session.id)
        .collect::<Vec<_>>();
    for session_id in session_ids {
        // 浏览器可能恰好已经自行结束；这时无需把取消接口变成失败。
        let _ = state.browser_gateway.cancel_session(&session_id).await;
    }
    crate::external_cdp::close_task_targets(task_id).await;
}

pub async fn settings(state: web::Data<ServerState>) -> impl Responder {
    let saved = state.store.settings();
    HttpResponse::Ok().json(serde_json::json!({ "data": saved }))
}

pub async fn update_settings(
    state: web::Data<ServerState>,
    payload: web::Json<SettingsUpdate>,
) -> impl Responder {
    let previous_interval = state.store.settings().check_interval_minutes;
    let data = state.store.update_settings(payload.into_inner());
    if data.check_interval_minutes != previous_interval {
        state.account_refresh.notify_schedule_changed();
    }
    HttpResponse::Ok().json(serde_json::json!({ "data": data }))
}

pub(crate) fn api_key_allowed(state: &ServerState, request: &HttpRequest) -> bool {
    api_key_from_request(request).is_some_and(|bearer| state.store.api_key_allowed(&bearer))
}

pub(crate) fn api_key_from_request(request: &HttpRequest) -> Option<String> {
    request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
