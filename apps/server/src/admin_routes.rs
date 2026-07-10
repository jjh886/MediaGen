use std::time::Duration;

use actix_web::{web, HttpResponse, Responder};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use rand::{rngs::OsRng, RngCore};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::net_client::{http_client, proxy_url_with_auth, proxy_url_without_auth};
use crate::store::{ApiKeyRecord, AuditLogRecord, ProxyRecord};
use crate::ServerState;

const DEFAULT_PROXY_TEST_URL: &str = "https://www.gstatic.com/generate_204";

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/api-keys", web::get().to(api_keys))
        .route("/api/api-keys", web::post().to(create_api_key))
        .route("/api/api-keys/{id}", web::delete().to(delete_api_key))
        .route("/api/proxies", web::get().to(proxies))
        .route("/api/proxies", web::post().to(create_proxy))
        .route("/api/proxies/{id}/test", web::post().to(test_proxy))
        .route("/api/proxies/{id}", web::delete().to(delete_proxy))
        .route("/api/logs", web::get().to(logs))
        .route("/api/logs/audit", web::get().to(logs))
        .route("/api/logs/clear", web::post().to(clear_logs))
        .route("/api/cache/clear", web::post().to(clear_cache))
        .route(
            "/api/accounts/batch-register",
            web::post().to(batch_register),
        );
}

pub async fn api_keys(state: web::Data<ServerState>) -> impl Responder {
    let data = state
        .store
        .list_api_keys()
        .into_iter()
        .map(|item| {
            json!({
                "id": item.id,
                "name": item.name,
                "masked_key": format!("{}****{}", item.key_prefix, item.key_suffix),
                "rpm_limit": item.rpm_limit,
                "daily_limit": item.daily_limit,
                "expires_at": item.expires_at,
                "created_at": item.created_at,
            })
        })
        .collect::<Vec<_>>();
    HttpResponse::Ok().json(json!({ "data": data }))
}

#[derive(Deserialize)]
pub struct ApiKeyPayload {
    name: String,
}

pub async fn create_api_key(
    state: web::Data<ServerState>,
    payload: web::Json<ApiKeyPayload>,
) -> impl Responder {
    let payload = payload.into_inner();
    let api_key = generate_api_key();
    let record = state.store.create_api_key(payload.name, api_key.clone());
    state.store.add_audit_log(
        "api",
        "info",
        "新增 API Key".to_string(),
        record.name.clone(),
        None,
    );
    HttpResponse::Ok().json(json!({ "data": api_key_create_response(&record, &api_key) }))
}

pub async fn delete_api_key(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    if state.store.delete_api_key(&path.into_inner()) {
        HttpResponse::Ok().json(json!({ "ok": true }))
    } else {
        not_found("API Key 不存在")
    }
}

pub async fn proxies(state: web::Data<ServerState>) -> impl Responder {
    let data = state
        .store
        .list_proxies()
        .into_iter()
        .map(proxy_response)
        .collect::<Vec<_>>();
    HttpResponse::Ok().json(json!({ "data": data }))
}

#[derive(Deserialize)]
pub struct ProxyPayload {
    name: String,
    server: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

pub async fn create_proxy(
    state: web::Data<ServerState>,
    payload: web::Json<ProxyPayload>,
) -> impl Responder {
    let payload = payload.into_inner();
    if !valid_proxy_server(&payload.server) {
        return bad_request("代理地址必须是 socks5:// 或 socks5h:// 开头");
    }
    let record = state.store.create_proxy(ProxyRecord {
        id: format!("proxy-{}", Uuid::new_v4()),
        name: first_non_empty(&payload.name, "SOCKS5 代理"),
        server: payload.server.trim().to_string(),
        username: payload.username.trim().to_string(),
        password: payload.password,
        enabled: payload.enabled,
        created_at: Utc::now().to_rfc3339(),
    });
    HttpResponse::Ok().json(json!({ "data": proxy_response(record) }))
}

pub async fn delete_proxy(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    if state.store.delete_proxy(&path.into_inner()) {
        HttpResponse::Ok().json(json!({ "ok": true }))
    } else {
        not_found("代理不存在")
    }
}

#[derive(Deserialize)]
pub struct ProxyTestPayload {
    url: Option<String>,
}

pub async fn test_proxy(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    payload: web::Json<ProxyTestPayload>,
) -> impl Responder {
    let Some(proxy) = state.store.proxy_by_id(&path.into_inner()) else {
        return not_found("代理不存在");
    };
    if !proxy.enabled {
        return bad_request("代理已停用，启用后才能测试");
    }

    let target = proxy_test_target(payload.url.as_deref());
    if !valid_proxy_test_target(&target) {
        return bad_request("测试地址必须是 http:// 或 https:// 开头");
    }

    let proxy_url = proxy_url_with_auth(&proxy.server, &proxy.username, &proxy.password);
    match run_proxy_test(&target, &proxy_url).await {
        Ok(status) => HttpResponse::Ok()
            .json(json!({ "data": { "ok": true, "status": status, "url": target } })),
        Err(error) => HttpResponse::BadGateway().json(json!({
            "error": sanitize_proxy_error(&error, &proxy_url),
        })),
    }
}

#[derive(Deserialize)]
pub struct LogQuery {
    module: Option<String>,
    level: Option<String>,
    keyword: Option<String>,
    task_id: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

pub async fn logs(state: web::Data<ServerState>, query: web::Query<LogQuery>) -> impl Responder {
    let data = filter_logs(state.store.list_audit_logs(), &query.into_inner());
    HttpResponse::Ok().json(json!({ "data": data }))
}

pub async fn clear_logs(state: web::Data<ServerState>) -> impl Responder {
    state.store.clear_audit_logs();
    HttpResponse::Ok().json(json!({ "ok": true }))
}

pub async fn clear_cache(state: web::Data<ServerState>) -> impl Responder {
    state.store.add_audit_log(
        "engine",
        "warn",
        "清理缓存".to_string(),
        "缓存清理尚未接入真实文件清理，前端应禁用该入口".to_string(),
        None,
    );
    HttpResponse::NotImplemented().json(json!({ "error": "缓存清理暂未接入真实文件清理" }))
}

#[derive(Deserialize)]
pub struct BatchRegisterPayload {
    platform: String,
    count: u32,
    #[allow(dead_code)]
    password: String,
    #[allow(dead_code)]
    birthday: String,
}

pub async fn batch_register(
    state: web::Data<ServerState>,
    payload: web::Json<BatchRegisterPayload>,
) -> impl Responder {
    let payload = payload.into_inner();
    let enabled = state
        .plugin_loader
        .get_plugin(&payload.platform)
        .and_then(|manifest| manifest.extra.get("registration"))
        .and_then(|value| value.get("enabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !enabled {
        return HttpResponse::NotImplemented().json(json!({
            "error": "该平台插件暂未开放自动注册，请使用添加账号登录。"
        }));
    }
    state.store.add_audit_log(
        "registration",
        "info",
        "创建批量注册任务".to_string(),
        format!("platform={}, count={}", payload.platform, payload.count),
        None,
    );
    HttpResponse::Accepted().json(json!({ "ok": true }))
}

fn bad_request(message: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(json!({ "error": message }))
}

fn not_found(message: &str) -> HttpResponse {
    HttpResponse::NotFound().json(json!({ "error": message }))
}

fn valid_proxy_server(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value.trim()) else {
        return false;
    };
    matches!(url.scheme(), "socks5" | "socks5h")
        && url.host_str().is_some_and(|host| !host.trim().is_empty())
        && url.port().is_some()
        && matches!(url.path(), "" | "/")
}

fn proxy_test_target(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(DEFAULT_PROXY_TEST_URL)
        .to_string()
}

fn valid_proxy_test_target(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value.trim()) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
}

async fn run_proxy_test(target: &str, proxy_url: &str) -> Result<u16, String> {
    let client = http_client(proxy_url)?;
    let response = tokio::time::timeout(
        Duration::from_secs(8),
        client
            .get(target)
            .header(reqwest::header::USER_AGENT, "MediaGen proxy test")
            .send(),
    )
    .await
    .map_err(|_| "代理测试超时，请检查代理地址、端口和服务器网络".to_string())?
    .map_err(|error| format!("代理测试失败: {error}"))?;
    Ok(response.status().as_u16())
}

fn sanitize_proxy_error(error: &str, proxy_url: &str) -> String {
    let public_proxy_url = proxy_url_without_auth(proxy_url);
    error.replace(proxy_url, &public_proxy_url)
}

fn default_enabled() -> bool {
    true
}

fn first_non_empty(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn proxy_response(proxy: ProxyRecord) -> Value {
    json!({
        "id": proxy.id,
        "name": proxy.name,
        "server": proxy.server,
        "username": proxy.username,
        "password_configured": !proxy.password.is_empty(),
        "enabled": proxy.enabled,
        "created_at": proxy.created_at,
    })
}

fn generate_api_key() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("mg_{}", URL_SAFE_NO_PAD.encode(bytes))
}

fn api_key_create_response(record: &ApiKeyRecord, key: &str) -> Value {
    json!({
        "id": record.id,
        "key": key,
        "masked_key": format!("{}****{}", record.key_prefix, record.key_suffix),
    })
}

fn filter_logs(mut logs: Vec<AuditLogRecord>, query: &LogQuery) -> Vec<AuditLogRecord> {
    logs.retain(|log| log_matches(log, query));
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    logs.into_iter().skip(offset).take(limit).collect()
}

fn log_matches(log: &AuditLogRecord, query: &LogQuery) -> bool {
    matches_text(&query.module, &log.module)
        && matches_text(&query.level, &log.level)
        && matches_task(&query.task_id, log.task_id.as_deref())
        && matches_keyword(&query.keyword, log)
}

fn matches_text(expected: &Option<String>, actual: &str) -> bool {
    expected
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none_or(|value| value == actual)
}

fn matches_task(expected: &Option<String>, actual: Option<&str>) -> bool {
    expected
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none_or(|value| actual == Some(value))
}

fn matches_keyword(expected: &Option<String>, log: &AuditLogRecord) -> bool {
    let Some(keyword) = expected
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    let keyword = keyword.to_lowercase();
    log.summary.to_lowercase().contains(&keyword) || log.detail.to_lowercase().contains(&keyword)
}
