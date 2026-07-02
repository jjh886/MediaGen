use actix_web::{web, HttpRequest, HttpResponse, Responder};
use base64::Engine;
use chrono::Utc;
use mediagen_core::{AdapterMode, ModelDef, PluginManifest};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::flow_runner::FlowRunner;
use crate::image_multipart::parse_image_request;
use crate::image_request::{ImageEndpoint, ImageFlowRequest, OpenAiImageRequest};
use crate::js_flow_runner::JsFlowRunner;
use crate::net_client::http_client;
use crate::routes::{api_key_allowed, api_key_from_request};
use crate::store::{AccountRecord, TaskRecord};
use crate::ServerState;

pub async fn generate_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    body: web::Json<OpenAiImageRequest>,
) -> impl Responder {
    handle_json_image(state, request, body.into_inner(), ImageEndpoint::Generation).await
}

pub async fn edit_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> impl Responder {
    handle_payload_image(state, request, payload, ImageEndpoint::Edit).await
}

pub async fn variation_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> impl Responder {
    handle_payload_image(state, request, payload, ImageEndpoint::Variation).await
}

async fn handle_json_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    mut body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
) -> HttpResponse {
    if !api_key_allowed(&state, &request) {
        return openai_error(401, "authentication_error", "invalid api key");
    }
    let caller_key = api_key_from_request(&request);
    body.apply_endpoint_defaults(endpoint);
    handle_image_request(state, body, endpoint, caller_key).await
}

async fn handle_payload_image(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
    endpoint: ImageEndpoint,
) -> HttpResponse {
    if !api_key_allowed(&state, &request) {
        return openai_error(401, "authentication_error", "invalid api key");
    }
    let caller_key = api_key_from_request(&request);
    let mut body = match parse_image_request(request, payload).await {
        Ok(body) => body,
        Err((status, code, message)) => return openai_error(status, code, &message),
    };
    body.apply_endpoint_defaults(endpoint);
    handle_image_request(state, body, endpoint, caller_key).await
}

async fn handle_image_request(
    state: web::Data<ServerState>,
    body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
    caller_key: Option<String>,
) -> HttpResponse {
    if let Err((status, code, message)) = body.validate(endpoint) {
        return openai_error(status, code, &message);
    }
    if body.stream == Some(true) {
        return openai_error(
            400,
            "unsupported_parameter",
            "服务端 Web 版当前先返回 JSON 结果，stream=true 请后续再开启。",
        );
    }
    match run_image_flow(state, body, endpoint, caller_key).await {
        Ok(body) => HttpResponse::Ok().json(body),
        Err((status, code, message)) => openai_error(status, code, &message),
    }
}

async fn run_image_flow(
    state: web::Data<ServerState>,
    body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
    caller_key: Option<String>,
) -> Result<Value, (u16, &'static str, String)> {
    let context = resolve_image_flow_context(&state, &body, caller_key.as_deref())?;
    state.store.insert_task(new_task(
        &context.task_id,
        context.manifest,
        &context.model.model_id,
        &body,
        &context.account.id,
    ));

    let (flow_request, ref_image_locals) =
        prepare_flow_request(&body, context.model, endpoint, &context.account.proxy_url).await;
    let result = execute_image_flow(&state, context.manifest, &context.account, flow_request).await;
    cleanup_temp_files(&ref_image_locals).await;

    match result {
        Ok(result) => {
            complete_image_task(&state, &context.task_id, &context.account.proxy_url, result).await
        }
        Err(message) => {
            state.store.fail_task(&context.task_id, message.clone());
            Err((502, "plugin_execution_failed", message))
        }
    }
}

struct ImageFlowContext<'a> {
    manifest: &'a PluginManifest,
    model: &'a ModelDef,
    account: AccountRecord,
    task_id: String,
}

fn resolve_image_flow_context<'a>(
    state: &'a web::Data<ServerState>,
    body: &OpenAiImageRequest,
    caller_key: Option<&str>,
) -> Result<ImageFlowContext<'a>, (u16, &'static str, String)> {
    let (manifest, model) = state
        .plugin_loader
        .resolve_model(&body.model)
        .ok_or_else(|| {
            (
                404,
                "model_not_found",
                format!("模型 {} 不存在", body.model),
            )
        })?;
    let account = state
        .store
        .first_available_account_for_key(&manifest.platform_id, caller_key)
        .ok_or_else(|| {
            (
                409,
                "account_unavailable",
                format!("{} 没有可用账号，请先添加账号。", manifest.display_name),
            )
        })?;
    Ok(ImageFlowContext {
        manifest,
        model,
        account,
        task_id: format!("task-{}", Uuid::new_v4()),
    })
}

async fn prepare_flow_request(
    body: &OpenAiImageRequest,
    model: &ModelDef,
    endpoint: ImageEndpoint,
    proxy_url: &str,
) -> (ImageFlowRequest, Vec<String>) {
    let ref_urls = body.reference_urls();
    let ref_dataurls = fetch_ref_images_as_data_urls(&ref_urls, proxy_url).await;
    let ref_image_locals = fetch_ref_images_as_local_files(&ref_urls, proxy_url).await;
    let mask_dataurl = fetch_optional_data_url(
        body.mask
            .as_ref()
            .and_then(|item| item.image_url.as_deref()),
        proxy_url,
    )
    .await;
    let mut flow_request = body.to_flow_request(
        model.display_name.clone(),
        endpoint,
        ref_dataurls,
        mask_dataurl,
    );
    flow_request.ref_image_locals = ref_image_locals.clone();
    (flow_request, ref_image_locals)
}

async fn execute_image_flow(
    state: &web::Data<ServerState>,
    manifest: &PluginManifest,
    account: &AccountRecord,
    flow_request: ImageFlowRequest,
) -> Result<Value, String> {
    match manifest.adapter_mode {
        AdapterMode::Browser | AdapterMode::Hybrid => {
            FlowRunner::new(state.plugin_loader.clone(), state.browser_gateway.clone())
                .run_image_flow(manifest, account, flow_request)
                .await
        }
        AdapterMode::Api => {
            JsFlowRunner::new(state.plugin_loader.clone())
                .run_image_flow(manifest, account, flow_request)
                .await
        }
    }
}

async fn complete_image_task(
    state: &web::Data<ServerState>,
    task_id: &str,
    proxy_url: &str,
    result: Value,
) -> Result<Value, (u16, &'static str, String)> {
    let images = collect_b64_images(&result, proxy_url)
        .await
        .map_err(|message| {
            state.store.fail_task(task_id, message.clone());
            (502, "image_result_error", message)
        })?;
    state.store.complete_task(task_id, result);
    Ok(json!({
        "created": Utc::now().timestamp(),
        "data": images.into_iter().map(|b64| json!({ "b64_json": b64 })).collect::<Vec<_>>(),
    }))
}

fn new_task(
    task_id: &str,
    manifest: &mediagen_core::PluginManifest,
    model_id: &str,
    body: &OpenAiImageRequest,
    account_id: &str,
) -> TaskRecord {
    TaskRecord {
        id: task_id.to_string(),
        platform: manifest.platform_id.clone(),
        task_kind: "image".to_string(),
        model_id: model_id.to_string(),
        prompt: body.prompt_text(),
        account_id: Some(account_id.to_string()),
        status: "processing".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: Utc::now().to_rfc3339(),
        completed_at: None,
    }
}

async fn collect_b64_images(result: &Value, proxy_url: &str) -> Result<Vec<String>, String> {
    let mut images = Vec::new();
    if let Some(items) = result.get("images").and_then(Value::as_array) {
        for item in items {
            if let Some(url) = item.get("url").and_then(Value::as_str) {
                images.push(image_url_to_b64(url, proxy_url).await?);
            }
        }
    }
    if images.is_empty() {
        if let Some(url) = result
            .get("download_url")
            .or_else(|| result.get("url"))
            .and_then(Value::as_str)
        {
            images.push(image_url_to_b64(url, proxy_url).await?);
        }
    }
    if images.is_empty() {
        return Err("插件没有返回可下载图片".to_string());
    }
    Ok(images)
}

async fn fetch_ref_images_as_data_urls(urls: &[String], proxy_url: &str) -> Vec<String> {
    let mut out = Vec::new();
    for url in urls {
        if url.starts_with("data:image/") {
            out.push(url.clone());
        } else if let Ok(data_url) = download_image_as_data_url(url, proxy_url).await {
            out.push(data_url);
        }
    }
    out
}

async fn fetch_ref_images_as_local_files(urls: &[String], proxy_url: &str) -> Vec<String> {
    let mut out = Vec::new();
    for url in urls {
        match image_url_to_bytes(url, proxy_url).await {
            Ok((bytes, ext)) => {
                let path =
                    std::env::temp_dir().join(format!("mediagen_ref_{}.{}", Uuid::new_v4(), ext));
                if tokio::fs::write(&path, bytes).await.is_ok() {
                    out.push(path.to_string_lossy().to_string());
                }
            }
            Err(message) => {
                eprintln!("[mediagen-server] 参考图写入临时文件失败: {message}");
            }
        }
    }
    out
}

async fn cleanup_temp_files(paths: &[String]) {
    for path in paths {
        let _ = tokio::fs::remove_file(path).await;
    }
}

async fn fetch_optional_data_url(url: Option<&str>, proxy_url: &str) -> Option<String> {
    let url = url?;
    if url.starts_with("data:image/") {
        return Some(url.to_string());
    }
    download_image_as_data_url(url, proxy_url).await.ok()
}

async fn download_image_as_data_url(url: &str, proxy_url: &str) -> Result<String, String> {
    let (bytes, ext) = image_url_to_bytes(url, proxy_url).await?;
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    };
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

async fn image_url_to_bytes(url: &str, proxy_url: &str) -> Result<(bytes::Bytes, String), String> {
    if let Some((head, encoded)) = url.split_once(";base64,") {
        if head.starts_with("data:image/") {
            let ext = head
                .trim_start_matches("data:image/")
                .split(';')
                .next()
                .unwrap_or("png")
                .replace("jpeg", "jpg");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|err| format!("data URL 图片解码失败: {err}"))?;
            return Ok((bytes::Bytes::from(bytes), ext));
        }
    }
    let response = http_client(proxy_url)?
        .get(url)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("图片下载失败: HTTP {}", response.status()));
    }
    let ext = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(extension_from_mime)
        .unwrap_or("png")
        .to_string();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    Ok((bytes, ext))
}

fn extension_from_mime(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    }
}

async fn image_url_to_b64(url: &str, proxy_url: &str) -> Result<String, String> {
    if let Some((_, b64)) = url.split_once(";base64,") {
        return Ok(b64.to_string());
    }
    let response = http_client(proxy_url)?
        .get(url)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("图片下载失败: HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn openai_error(status: u16, code: &'static str, message: &str) -> HttpResponse {
    HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status)
            .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
    )
    .json(json!({
        "error": {
            "message": message,
            "type": code,
            "code": code,
        }
    }))
}
