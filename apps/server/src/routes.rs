use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::access_control::mask_account_keys;
use crate::account_login;
use crate::net_client::proxy_url_without_auth;
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
}

pub async fn accounts(
    state: web::Data<ServerState>,
    query: web::Query<AccountQuery>,
) -> impl Responder {
    let data = state
        .store
        .list_accounts(query.platform.as_deref())
        .into_iter()
        .map(public_account)
        .collect::<Vec<_>>();
    HttpResponse::Ok().json(json!({ "data": data }))
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
    let mut checked = 0usize;
    let mut expired = 0usize;
    let mut inconclusive = 0usize;
    for account in state.store.list_accounts(None) {
        if account.login_state == "disabled" {
            continue;
        }
        let runtime_account = state.store.account_with_runtime_proxy(account.clone());
        match account_login::check_saved_account_login(&state, &runtime_account).await {
            Ok(profile) => {
                checked += 1;
                state
                    .store
                    .mark_login_check_result(&account.id, "available", Some(profile));
            }
            Err(message) => {
                checked += 1;
                if message.contains("没有配置登录检测") || message.contains("暂不支持服务端")
                {
                    inconclusive += 1;
                    continue;
                }
                expired += 1;
                state
                    .store
                    .mark_login_check_result(&account.id, "login_expired", None);
            }
        }
    }
    HttpResponse::Ok().json(serde_json::json!({
        "checked": checked,
        "expired": expired,
        "inconclusive": inconclusive
    }))
}

pub async fn tasks(
    state: web::Data<ServerState>,
    query: web::Query<AccountQuery>,
) -> impl Responder {
    let data = state.store.list_tasks(query.platform.as_deref());
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

async fn cancel_task_browser_sessions(state: &ServerState, task_id: &str) {
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
}

pub async fn settings(state: web::Data<ServerState>) -> impl Responder {
    let saved = state.store.settings();
    HttpResponse::Ok().json(serde_json::json!({ "data": saved }))
}

pub async fn update_settings(
    state: web::Data<ServerState>,
    payload: web::Json<SettingsUpdate>,
) -> impl Responder {
    let data = state.store.update_settings(payload.into_inner());
    HttpResponse::Ok().json(serde_json::json!({ "data": data }))
}

pub async fn models(state: web::Data<ServerState>, request: HttpRequest) -> impl Responder {
    if !api_key_allowed(&state, &request) {
        return HttpResponse::Unauthorized().json(serde_json::json!({
            "error": {
                "message": "invalid api key",
                "type": "authentication_error"
            }
        }));
    }

    let mut data: Vec<_> = state
        .plugin_loader
        .all_models()
        .into_iter()
        .map(|(manifest, model)| {
            serde_json::json!({
                "id": model.model_id,
                "object": "model",
                "created": 0,
                "owned_by": manifest.platform_id,
                "display_name": model.display_name,
                "task_kind": model.task_kind,
            })
        })
        .collect();

    data.extend(
        state
            .plugin_loader
            .model_aliases()
            .into_iter()
            .map(|(alias, manifest, model)| {
                serde_json::json!({
                    "id": alias,
                    "object": "model",
                    "created": 0,
                    "owned_by": manifest.platform_id,
                    "display_name": alias,
                    "task_kind": model.task_kind,
                    "alias_of": model.model_id,
                })
            }),
    );

    HttpResponse::Ok().json(serde_json::json!({
        "object": "list",
        "data": data
    }))
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
