use actix_web::{web, HttpResponse, Responder};
use mediagen_browser_gateway::{
    BrowserCdpRequest, BrowserClipboardRequest, BrowserGatewayError, BrowserHistoryRequest,
    BrowserInputRequest, BrowserNavigateRequest, BrowserResizeRequest, BrowserScreenshotRequest,
    BrowserSession, BrowserSessionCreateRequest, BrowserSessionPurpose,
};
use mediagen_core::PluginManifest;
use serde_json::{json, Value};

use crate::account_login;
use crate::browser_session_public::{public_browser_response, public_browser_session};
use crate::browser_session_setup::{
    prepare_browser_session_create_request, release_relogin_profile,
};
use crate::oauth_login;
use crate::routes::public_account;
use crate::value_utils::file_paths_from_value;
use crate::ServerState;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/browser-sessions")
            .route(
                "",
                web::get().to(crate::browser_session_list::list_browser_sessions),
            )
            .route("", web::post().to(create_browser_session))
            .route("/{id}", web::get().to(get_browser_session))
            .route("/{id}/cancel", web::post().to(cancel_browser_session))
            .route("/{id}/complete", web::post().to(complete_browser_session))
            .route("/{id}/navigate", web::post().to(navigate_browser_session))
            .route("/{id}/history", web::post().to(browser_session_history))
            .route("/{id}/resize", web::post().to(resize_browser_session))
            .route(
                "/{id}/screenshot",
                web::post().to(screenshot_browser_session),
            )
            .route("/{id}/evaluate", web::post().to(evaluate_browser_session))
            .route("/{id}/click", web::post().to(click_browser_session))
            .route("/{id}/type", web::post().to(type_browser_session))
            .route("/{id}/upload", web::post().to(upload_browser_session))
            .route("/{id}/download", web::post().to(download_browser_session))
            .route("/{id}/cdp", web::post().to(cdp_browser_session))
            .route("/{id}/input", web::post().to(input_browser_session))
            .route("/{id}/clipboard", web::post().to(clipboard_browser_session))
            .route(
                "/{id}/events",
                web::get().to(crate::browser_session_events::browser_session_events),
            )
            .route(
                "/{id}/stream",
                web::get().to(crate::browser_session_stream::browser_session_stream),
            ),
    );
}

pub async fn navigate_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<BrowserNavigateRequest>,
) -> impl Responder {
    match state
        .browser_gateway
        .navigate_session(&path.into_inner(), request.into_inner())
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn browser_session_history(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<BrowserHistoryRequest>,
) -> impl Responder {
    match state
        .browser_gateway
        .history_session(&path.into_inner(), request.into_inner())
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn create_browser_session(
    state: web::Data<ServerState>,
    request: web::Json<BrowserSessionCreateRequest>,
) -> impl Responder {
    let mut request = request.into_inner();
    if let Err(response) = prepare_browser_session_create_request(&state, &mut request) {
        return response;
    }
    if let Some(manifest) = state.plugin_loader.get_plugin(&request.platform) {
        if let Err(message) = oauth_login::prepare_oauth_request(manifest, &mut request) {
            return HttpResponse::BadRequest().json(json!({ "ok": false, "error": message }));
        }
    }
    if let Err(err) = release_relogin_profile(&state, &request).await {
        return gateway_error_response(err);
    }
    if let Some(manifest) = state.plugin_loader.get_plugin(&request.platform) {
        if let Err(message) =
            crate::plugin_login_prepare::prepare(&state, manifest, &mut request).await
        {
            return HttpResponse::BadRequest().json(json!({"ok": false, "error": message}));
        }
    }
    let oauth_state = oauth_login::oauth_meta(&request.metadata).map(|meta| meta.state);
    if let Some(oauth_state) = oauth_state.as_deref() {
        if let Err(message) = state.oauth_logins.start_login(state.clone(), oauth_state) {
            return HttpResponse::Conflict().json(json!({ "ok": false, "error": message }));
        }
    }
    match state.browser_gateway.create_session(request).await {
        Ok(response) => HttpResponse::Ok().json(public_browser_response(response)),
        Err(err) => {
            if let Some(oauth_state) = oauth_state {
                state.oauth_logins.finish_login(&oauth_state).await;
            }
            gateway_error_response(err)
        }
    }
}

pub async fn get_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    match state.browser_gateway.get_session(&path.into_inner()) {
        Ok(session) => HttpResponse::Ok().json(serde_json::json!({
            "ok": true,
            "session": public_browser_session(session),
        })),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn cancel_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    let session_id = path.into_inner();
    let oauth_state = state
        .browser_gateway
        .get_session(&session_id)
        .ok()
        .and_then(|session| oauth_login::oauth_meta(&session.metadata))
        .map(|meta| meta.state);
    let result = state.browser_gateway.cancel_session(&session_id).await;
    if let Some(oauth_state) = oauth_state {
        state.oauth_logins.finish_login(&oauth_state).await;
    }
    match result {
        Ok(response) => HttpResponse::Ok().json(public_browser_response(response)),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn complete_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    let session_id = path.into_inner();
    let current_session = match state.browser_gateway.get_session(&session_id) {
        Ok(session) => session,
        Err(err) => return gateway_error_response(err),
    };
    let verification_context = verification_completion_context(&current_session);
    let (login_profile, manifest) = match prepare_login_completion(&state, &current_session).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let oauth_state = oauth_login::oauth_meta(&current_session.metadata).map(|meta| meta.state);

    match state.browser_gateway.complete_session(&session_id).await {
        Ok(response) => {
            if let Some(oauth_state) = oauth_state {
                state.oauth_logins.finish_login(&oauth_state).await;
            }
            if let Some((task_id, account_id)) = verification_context {
                finish_human_verification(&state, &task_id, &account_id);
                return HttpResponse::Ok().json(serde_json::json!({
                    "ok": true,
                    "session": public_browser_session(response.session),
                    "account": account_id
                        .and_then(|id| state.store.account_by_id(&id))
                        .map(public_account),
                }));
            }
            let account = match (manifest.as_ref(), login_profile) {
                (Some(manifest), Some(profile)) => state
                    .store
                    .upsert_from_browser_session_with_profile(&response.session, manifest, profile),
                _ => state.store.upsert_from_browser_session(&response.session),
            };
            let public_account = account.map(public_account);
            HttpResponse::Ok().json(serde_json::json!({
                "ok": true,
                "session": public_browser_session(response.session),
                "account": public_account,
            }))
        }
        Err(err) => gateway_error_response(err),
    }
}

/// 登录会话完成前统一校验登录态并采集插件声明的凭据，避免完成会话后再访问已关闭的浏览器。
async fn prepare_login_completion(
    state: &ServerState,
    session: &BrowserSession,
) -> Result<
    (
        Option<account_login::AccountLoginProfile>,
        Option<PluginManifest>,
    ),
    HttpResponse,
> {
    let profile = match session.purpose {
        BrowserSessionPurpose::AddAccount | BrowserSessionPurpose::ReloginAccount => Some(
            account_login::verify_browser_session_login(state, session)
                .await
                .map_err(login_conflict_response)?,
        ),
        _ => None,
    };
    let manifest = profile
        .as_ref()
        .and_then(|_| state.plugin_loader.get_plugin(&session.platform).cloned());
    if let Some(manifest) = manifest.as_ref() {
        crate::account_login_credentials::capture_if_configured(state, session, manifest)
            .await
            .map_err(login_conflict_response)?;
    }
    Ok((profile, manifest))
}

fn login_conflict_response(message: String) -> HttpResponse {
    HttpResponse::Conflict().json(serde_json::json!({
        "ok": false,
        "error": message,
    }))
}

fn verification_completion_context(session: &BrowserSession) -> Option<(String, Option<String>)> {
    if session.purpose != BrowserSessionPurpose::HumanVerification {
        return None;
    }
    let task_id = session.task_id.as_deref()?.trim();
    if task_id.is_empty() {
        return None;
    }
    Some((task_id.to_string(), session.account_id.clone()))
}

fn finish_human_verification(state: &ServerState, task_id: &str, account_id: &Option<String>) {
    if let Some(id) = account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        state.store.mark_account_state(id, "available");
    }
    state.store.resume_task_after_verification(task_id);
    state.store.add_task_event(
        task_id,
        "info",
        "人工验证已完成，任务已恢复为可重试状态".to_string(),
    );
    state.store.add_audit_log(
        "account",
        "info",
        "人工验证完成".to_string(),
        format!("task={task_id}"),
        Some(task_id.to_string()),
    );
}

pub async fn resize_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<BrowserResizeRequest>,
) -> impl Responder {
    match state
        .browser_gateway
        .resize_session(&path.into_inner(), request.into_inner())
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn screenshot_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<BrowserScreenshotRequest>,
) -> impl Responder {
    match state
        .browser_gateway
        .screenshot_session(&path.into_inner(), request.into_inner())
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn evaluate_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<Value>,
) -> impl Responder {
    let script = request["script"].as_str().unwrap_or_default();
    if script.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({ "ok": false, "error": "缺少 script" }));
    }
    let await_promise = request["await_promise"]
        .as_bool()
        .or_else(|| request["awaitPromise"].as_bool())
        .unwrap_or(false);
    match state
        .browser_gateway
        .evaluate_script(&path.into_inner(), script, await_promise)
        .await
    {
        Ok(data) => HttpResponse::Ok().json(json!({ "ok": true, "data": data })),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn click_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<Value>,
) -> impl Responder {
    let session_id = path.into_inner();
    let result = if let Some(selector) = request["selector"].as_str() {
        state
            .browser_gateway
            .click_selector(&session_id, selector)
            .await
    } else if let Some(text) = request["text"].as_str() {
        state
            .browser_gateway
            .click_text(
                &session_id,
                text,
                request["tag"].as_str().unwrap_or("*"),
                request["exact"].as_bool().unwrap_or(false),
            )
            .await
    } else {
        return HttpResponse::BadRequest().json(json!({
            "ok": false,
            "error": "缺少 selector 或 text"
        }));
    };
    match result {
        Ok(data) => HttpResponse::Ok().json(json!({ "ok": true, "data": data })),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn type_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<Value>,
) -> impl Responder {
    let selector = request["selector"].as_str().unwrap_or_default();
    let text = request["text"].as_str().unwrap_or_default();
    if selector.is_empty() {
        return HttpResponse::BadRequest().json(json!({ "ok": false, "error": "缺少 selector" }));
    }
    match state
        .browser_gateway
        .type_text(
            &path.into_inner(),
            selector,
            text,
            request["clear_first"].as_bool().unwrap_or(false),
        )
        .await
    {
        Ok(data) => HttpResponse::Ok().json(json!({ "ok": true, "data": data })),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn upload_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<Value>,
) -> impl Responder {
    let selector = request["selector"].as_str().unwrap_or_default();
    if selector.is_empty() {
        return HttpResponse::BadRequest().json(json!({ "ok": false, "error": "缺少 selector" }));
    }
    let files = file_paths_from_value(&request["file_paths"]);
    match state
        .browser_gateway
        .set_file_input(&path.into_inner(), selector, files)
        .await
    {
        Ok(data) => HttpResponse::Ok().json(json!({ "ok": true, "data": data })),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn download_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<Value>,
) -> impl Responder {
    let url = request["url"].as_str().unwrap_or_default();
    if url.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({ "ok": false, "error": "缺少 url" }));
    }
    match state
        .browser_gateway
        .download_in_browser(&path.into_inner(), url)
        .await
    {
        Ok(data) => HttpResponse::Ok().json(json!({ "ok": true, "data": data })),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn cdp_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<BrowserCdpRequest>,
) -> impl Responder {
    match state
        .browser_gateway
        .send_cdp_command(&path.into_inner(), request.into_inner())
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn input_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<BrowserInputRequest>,
) -> impl Responder {
    match state
        .browser_gateway
        .dispatch_input(&path.into_inner(), request.into_inner())
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn clipboard_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: web::Json<BrowserClipboardRequest>,
) -> impl Responder {
    match state
        .browser_gateway
        .clipboard(&path.into_inner(), request.into_inner())
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => gateway_error_response(err),
    }
}

pub(crate) fn gateway_error_response(err: BrowserGatewayError) -> HttpResponse {
    let status = match err {
        BrowserGatewayError::SessionNotFound(_) => actix_web::http::StatusCode::NOT_FOUND,
        BrowserGatewayError::SessionClosed(_) => actix_web::http::StatusCode::CONFLICT,
        BrowserGatewayError::BrowserRuntimeUnavailable(_) => {
            actix_web::http::StatusCode::PRECONDITION_FAILED
        }
        BrowserGatewayError::BrowserLaunch(_) | BrowserGatewayError::Cdp(_) => {
            actix_web::http::StatusCode::BAD_GATEWAY
        }
        BrowserGatewayError::StatePoisoned => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
    };

    HttpResponse::build(status).json(serde_json::json!({
        "ok": false,
        "error": err.to_string(),
    }))
}
