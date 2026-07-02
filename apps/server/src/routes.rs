use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;

use crate::access_control::{key_grants, settings_key_allowed};
use crate::account_login;
use crate::browser_runtime;
use crate::{
    store::{AccountUpdate, SettingsUpdate},
    ServerState,
};

pub async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "service": "mediagen-server"
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
    let data = state.store.list_accounts(query.platform.as_deref());
    HttpResponse::Ok().json(serde_json::json!({ "data": data }))
}

pub async fn account_groups(state: web::Data<ServerState>) -> impl Responder {
    let mut groups = std::collections::BTreeMap::<String, usize>::new();
    for account in state.store.list_accounts(None) {
        let name = if account.group_name.trim().is_empty() {
            "默认分组"
        } else {
            account.group_name.trim()
        };
        *groups.entry(name.to_string()).or_default() += 1;
    }
    for (_, granted_groups) in key_grants(&state.store.settings().api_keys) {
        for group in granted_groups {
            groups.entry(group).or_default();
        }
    }
    let data = groups
        .into_iter()
        .map(|(name, account_count)| {
            serde_json::json!({ "name": name, "account_count": account_count })
        })
        .collect::<Vec<_>>();
    HttpResponse::Ok().json(serde_json::json!({ "data": data }))
}

pub async fn api_keys(state: web::Data<ServerState>) -> impl Responder {
    let data = key_grants(&state.store.settings().api_keys)
        .into_iter()
        .map(|(key, groups)| serde_json::json!({ "key": mask_key(&key), "groups": groups }))
        .collect::<Vec<_>>();
    HttpResponse::Ok().json(serde_json::json!({ "data": data }))
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
        Some(account) => HttpResponse::Ok().json(serde_json::json!({ "data": account })),
        None => HttpResponse::NotFound().json(serde_json::json!({ "error": "account not found" })),
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
        Some(account) => HttpResponse::Ok().json(serde_json::json!({ "data": account })),
        None => HttpResponse::NotFound().json(serde_json::json!({ "error": "account not found" })),
    }
}

pub async fn check_accounts_login(state: web::Data<ServerState>) -> impl Responder {
    let mut checked = 0usize;
    let mut expired = 0usize;
    let mut inconclusive = 0usize;
    for account in state.store.list_accounts(None) {
        if account.login_state == "disabled" {
            continue;
        }
        match account_login::check_saved_account_login(&state, &account).await {
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
    match state.store.cancel_task(&path.into_inner()) {
        Some(task) => HttpResponse::Ok().json(serde_json::json!({ "data": task })),
        None => HttpResponse::NotFound().json(serde_json::json!({ "error": "task not found" })),
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

pub async fn logs() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({ "data": [] }))
}

pub async fn browser_runtimes() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "data": [browser_runtime::detect_cloakbrowser()],
    }))
}

pub async fn browser_runtime_install_guide() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "runtime": "CloakBrowser",
        "status": "manual_or_packaged_install_required",
        "steps": browser_runtime::install_steps(),
        "notes": [
            "正式安装包会尽量随包携带当前平台的 CloakBrowser 二进制。",
            "如果没有随包携带，自动安装会下载到当前用户的 ~/.cloakbrowser 缓存目录。",
            "安装完成后点击重新检测即可进入应用。"
        ]
    }))
}

pub async fn install_browser_runtime() -> impl Responder {
    match browser_runtime::install_cloakbrowser() {
        Ok(runtime) => HttpResponse::Ok().json(serde_json::json!({ "data": runtime })),
        Err(message) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": message
        })),
    }
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
    let settings = state.store.settings();
    if settings.api_keys.trim().is_empty() {
        return false;
    }
    api_key_from_request(request)
        .is_some_and(|bearer| settings_key_allowed(&settings.api_keys, &bearer))
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

fn mask_key(key: &str) -> String {
    let chars = key.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "********".to_string();
    }
    format!(
        "{}****{}",
        chars.iter().take(4).collect::<String>(),
        chars
            .iter()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    )
}
