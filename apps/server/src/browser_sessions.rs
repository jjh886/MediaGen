use actix_web::{web, HttpResponse, Responder};
use mediagen_browser_gateway::{
    BrowserCdpRequest, BrowserClipboardRequest, BrowserGatewayError, BrowserHistoryRequest,
    BrowserInputRequest, BrowserNavigateRequest, BrowserResizeRequest, BrowserScreenshotRequest,
    BrowserSessionCreateRequest,
};
use serde_json::{json, Value};

use crate::account_login;
use crate::oauth_login;
use crate::value_utils::file_paths_from_value;
use crate::ServerState;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/browser-sessions")
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
    if let Some(manifest) = state.plugin_loader.get_plugin(&request.platform) {
        if let Err(message) = oauth_login::prepare_oauth_request(manifest, &mut request) {
            return HttpResponse::BadRequest().json(json!({ "ok": false, "error": message }));
        }
    }
    match state.browser_gateway.create_session(request).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn get_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    match state.browser_gateway.get_session(&path.into_inner()) {
        Ok(session) => HttpResponse::Ok().json(serde_json::json!({
            "ok": true,
            "session": session,
        })),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn cancel_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    match state
        .browser_gateway
        .cancel_session(&path.into_inner())
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
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
    let login_profile = match current_session.purpose {
        mediagen_browser_gateway::BrowserSessionPurpose::AddAccount
        | mediagen_browser_gateway::BrowserSessionPurpose::ReloginAccount => {
            match account_login::verify_browser_session_login(&state, &current_session).await {
                Ok(profile) => Some(profile),
                Err(message) => {
                    return HttpResponse::Conflict().json(serde_json::json!({
                        "ok": false,
                        "error": message,
                    }));
                }
            }
        }
        _ => None,
    };
    let manifest = login_profile.as_ref().and_then(|_| {
        state
            .plugin_loader
            .get_plugin(&current_session.platform)
            .cloned()
    });

    match state.browser_gateway.complete_session(&session_id).await {
        Ok(response) => {
            let account = match (manifest.as_ref(), login_profile) {
                (Some(manifest), Some(profile)) => state
                    .store
                    .upsert_from_browser_session_with_profile(&response.session, manifest, profile),
                _ => state.store.upsert_from_browser_session(&response.session),
            };
            HttpResponse::Ok().json(serde_json::json!({
                "ok": true,
                "session": response.session,
                "account": account,
            }))
        }
        Err(err) => gateway_error_response(err),
    }
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
