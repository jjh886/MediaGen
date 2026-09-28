use actix_web::{web, HttpResponse, Responder};
use mediagen_browser_gateway::{
    BrowserCdpRequest, BrowserClipboardRequest, BrowserHistoryRequest, BrowserInputRequest,
    BrowserNavigateRequest, BrowserResizeRequest, BrowserScreenshotRequest,
};
use serde_json::{json, Value};

use super::gateway_error_response;
use crate::value_utils::file_paths_from_value;
use crate::ServerState;

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
