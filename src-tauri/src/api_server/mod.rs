mod browser_lifecycle;
mod image;
mod image_account;
mod image_attempt;
mod image_capabilities;
mod image_events;
mod image_handler;
mod image_multipart;
mod image_output;
mod image_partials;
mod image_refs;
mod image_request;
mod image_sse;

use actix_web::http::header;
use actix_web::{web, App, HttpResponse, HttpServer};
use std::sync::Arc;

use crate::AppState;
pub(crate) use image_request::{
    ImageEndpoint, ImageGenContext, OpenAiImageRef, OpenAiImageRequest,
};

pub(crate) fn error_response(status: u16, code: &str, detail: &str) -> HttpResponse {
    let body = serde_json::json!({
        "error": {
            "message": detail,
            "type": "invalid_request_error",
            "param": serde_json::Value::Null,
            "code": code,
        },
    });
    HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status)
            .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
    )
    .json(body)
}

#[derive(Clone, Debug)]
pub struct ApiServerConfig {
    pub bind_host: String,
    pub ports: Vec<u16>,
    pub allowed_origins: Vec<String>,
}

impl ApiServerConfig {
    pub fn single_local(port: u16) -> Self {
        Self {
            bind_host: "127.0.0.1".to_string(),
            ports: vec![port],
            allowed_origins: vec!["*".to_string()],
        }
    }
}

pub async fn start_api_server(state: Arc<AppState>, port: u16) -> std::io::Result<()> {
    start_api_servers(state, ApiServerConfig::single_local(port)).await
}

pub async fn start_api_servers(
    state: Arc<AppState>,
    config: ApiServerConfig,
) -> std::io::Result<()> {
    let data = web::Data::from(state);
    let allowed_origins = Arc::new(config.allowed_origins);
    let bind_host = config.bind_host.clone();
    let ports = normalize_ports(config.ports);

    let mut listeners = Vec::new();
    for port in ports.iter().copied() {
        match std::net::TcpListener::bind((bind_host.as_str(), port)) {
            Ok(listener) => {
                tracing::info!(host = %bind_host, port, "API 服务器端口已绑定");
                listeners.push((port, listener));
            }
            Err(e) => {
                tracing::error!(host = %bind_host, port, error = %e, "API 服务器端口绑定失败");
            }
        }
    }

    if listeners.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("没有可用 API 监听端口: {}:{:?}", bind_host, ports),
        ));
    }

    let mut server = HttpServer::new(move || {
        let cors = build_cors(allowed_origins.clone());
        App::new()
            .wrap(cors)
            .app_data(data.clone())
            // 放宽 body 限制以容纳图片编辑接口的 multipart 图片字节（默认 256KB 太小，放到 50MB）。
            .app_data(web::PayloadConfig::new(50 * 1024 * 1024))
            .route("/v1/models", web::get().to(get_models))
            .route(
                "/v1/images/generations",
                web::post().to(image_handler::generate_image),
            )
            .route(
                "/v1/images/edits",
                web::post().to(image_handler::edit_image),
            )
            .route(
                "/v1/images/variations",
                web::post().to(image_handler::image_variation),
            )
    })
    .workers(4);

    for (_port, listener) in listeners {
        server = server.listen(listener)?;
    }

    server.run().await
}

fn build_cors(allowed_origins: Arc<Vec<String>>) -> actix_cors::Cors {
    actix_cors::Cors::default()
        .allowed_origin_fn(move |origin, _req_head| {
            origin
                .to_str()
                .ok()
                .map(|origin| is_allowed_origin(origin, &allowed_origins))
                .unwrap_or(false)
        })
        .allowed_methods(vec!["GET", "POST", "OPTIONS"])
        .allowed_headers(vec![
            header::AUTHORIZATION,
            header::ACCEPT,
            header::CONTENT_TYPE,
        ])
        .expose_headers(vec![header::CONTENT_TYPE])
        .supports_credentials()
        .max_age(3600)
}

fn is_allowed_origin(origin: &str, allowed_origins: &[String]) -> bool {
    if allowed_origins.iter().any(|allowed| allowed == "*") {
        return true;
    }
    if allowed_origins
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(origin))
    {
        return true;
    }
    is_localhost_origin(&origin.to_ascii_lowercase())
}

// is_localhost_origin 把本地/tauri 场景集中到一张白名单，避免主函数决策点过多。
fn is_localhost_origin(lower: &str) -> bool {
    const EXACT: &[&str] = &[
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
        "http://localhost",
        "https://localhost",
        "http://127.0.0.1",
        "https://127.0.0.1",
        "http://[::1]",
        "https://[::1]",
    ];
    const PREFIXES: &[&str] = &[
        "http://localhost:",
        "https://localhost:",
        "http://127.0.0.1:",
        "https://127.0.0.1:",
        "http://[::1]:",
        "https://[::1]:",
    ];
    EXACT.iter().any(|e| *e == lower) || PREFIXES.iter().any(|p| lower.starts_with(p))
}

fn normalize_ports(ports: Vec<u16>) -> Vec<u16> {
    let mut out = Vec::new();
    for port in ports {
        if port == 0 || out.contains(&port) {
            continue;
        }
        out.push(port);
    }
    out
}

async fn get_models(state: web::Data<AppState>) -> HttpResponse {
    let mut models: Vec<serde_json::Value> = state
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
    models.extend(state.plugin_loader.model_aliases().into_iter().map(
        |(alias, manifest, target)| {
            serde_json::json!({
                "id": alias,
                "object": "model",
                "created": 0,
                "owned_by": manifest.platform_id,
                "display_name": alias,
                "task_kind": target.task_kind,
                "alias_of": target.model_id,
            })
        },
    ));

    HttpResponse::Ok().json(serde_json::json!({
        "object": "list",
        "data": models,
    }))
}
