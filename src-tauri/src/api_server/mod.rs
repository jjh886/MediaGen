mod browser_lifecycle;
mod image;
mod image_account;
mod image_events;
mod image_handler;
mod image_multipart;
mod image_output;
mod image_partials;
mod image_refs;
mod image_sse;
mod imagex_proxy;

use actix_web::http::header;
use actix_web::{web, App, HttpResponse, HttpServer};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;

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
            // 放宽 body 限制以容纳 imagex_put_blob 的图片字节（默认 256KB 太小，放到 50MB）
            .app_data(web::PayloadConfig::new(50 * 1024 * 1024))
            .route("/v1/models", web::get().to(get_models))
            .route(
                "/api/internal/imagex_put_blob",
                web::post().to(imagex_proxy::imagex_put_blob),
            )
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
    models.extend(state.plugin_loader.openai_image_aliases().into_iter().map(
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageEndpoint {
    Generation,
    Edit,
}

impl ImageEndpoint {
    pub(crate) fn completed_event_type(self) -> &'static str {
        match self {
            Self::Generation => "image_generation.completed",
            Self::Edit => "image_edit.completed",
        }
    }
}

/// OpenAI Image API 风格请求体。
/// 生成接口接收 JSON；编辑接口同时接收 JSON 和 OpenAI SDK 默认的 multipart 文件上传。
#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct OpenAiImageRequest {
    #[serde(default)]
    pub model: String,
    pub prompt: Option<String>,
    #[serde(default)]
    pub n: Option<u32>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub output_format: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub partial_images: Option<u32>,
    #[serde(default)]
    pub output_compression: Option<u32>,
    #[serde(default)]
    pub moderation: Option<String>,
    #[serde(default)]
    pub input_fidelity: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub images: Vec<OpenAiImageRef>,
    #[serde(default)]
    pub mask: Option<OpenAiImageRef>,
    // 本地扩展字段：少量插件仍按比例选择平台选项，未传时由 size 自动映射。
    #[serde(default)]
    pub image_aspect_ratio: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
pub(crate) struct OpenAiImageRef {
    pub file_id: Option<String>,
    pub image_url: Option<String>,
}

impl OpenAiImageRequest {
    pub(crate) fn apply_endpoint_defaults(&mut self, endpoint: ImageEndpoint) {
        if endpoint == ImageEndpoint::Edit && self.model.trim().is_empty() {
            // OpenAI 的编辑接口 model 是可选项；本地默认落到兼容别名，方便标准 SDK 调用。
            self.model = "gpt-image-1".to_string();
        }
    }

    pub(crate) fn prompt_text(&self) -> String {
        self.prompt.clone().unwrap_or_default()
    }

    pub(crate) fn output_format(&self) -> String {
        self.output_format
            .clone()
            .unwrap_or_else(|| "png".to_string())
    }

    pub(crate) fn quality(&self) -> String {
        self.quality.clone().unwrap_or_else(|| "auto".to_string())
    }

    pub(crate) fn requested_size(&self) -> String {
        self.size.clone().unwrap_or_else(|| "auto".to_string())
    }

    pub(crate) fn max_images(&self) -> u32 {
        self.n.unwrap_or(1).clamp(1, 4)
    }

    pub(crate) fn reference_urls(&self) -> Vec<String> {
        self.images
            .iter()
            .filter_map(|item| item.image_url.clone())
            .filter(|url| !url.trim().is_empty())
            .collect()
    }

    pub(crate) fn image_aspect_ratio(&self) -> String {
        if let Some(ratio) = self
            .image_aspect_ratio
            .as_ref()
            .filter(|v| !v.trim().is_empty())
        {
            return ratio.clone();
        }
        match self.size.as_deref().unwrap_or("auto") {
            "1536x1024" => "16:9",
            "1024x1536" => "9:16",
            "1024x1024" => "1:1",
            _ => "1:1",
        }
        .to_string()
    }

    pub(crate) fn validate(
        &self,
        endpoint: ImageEndpoint,
    ) -> Result<(), (u16, &'static str, String)> {
        if self.model.trim().is_empty() {
            return Err((400, "invalid_request_error", "model 不能为空".to_string()));
        }
        if self.prompt_text().trim().is_empty() {
            return Err((400, "invalid_request_error", "prompt 不能为空".to_string()));
        }
        if self
            .response_format
            .as_deref()
            .is_some_and(|v| v != "b64_json")
        {
            return Err((
                400,
                "unsupported_parameter",
                "MediaGen 只返回 base64，请使用 response_format=b64_json 或省略该字段".to_string(),
            ));
        }
        if !matches!(self.output_format().as_str(), "png" | "jpeg" | "webp") {
            return Err((
                400,
                "invalid_request_error",
                "output_format 只能是 png、jpeg 或 webp".to_string(),
            ));
        }
        if !matches!(self.quality().as_str(), "auto" | "low" | "medium" | "high") {
            return Err((
                400,
                "invalid_request_error",
                "quality 只能是 auto、low、medium 或 high".to_string(),
            ));
        }
        if self
            .background
            .as_deref()
            .is_some_and(|v| !matches!(v, "auto" | "transparent" | "opaque"))
        {
            return Err((
                400,
                "invalid_request_error",
                "background 只能是 auto、transparent 或 opaque".to_string(),
            ));
        }
        if self
            .partial_images
            .is_some_and(|partial_images| partial_images > 3)
        {
            return Err((
                400,
                "invalid_request_error",
                "partial_images 必须在 0 到 3 之间".to_string(),
            ));
        }
        if self
            .output_compression
            .is_some_and(|compression| compression > 100)
        {
            return Err((
                400,
                "invalid_request_error",
                "output_compression 必须在 0 到 100 之间".to_string(),
            ));
        }
        if self
            .moderation
            .as_deref()
            .is_some_and(|v| !matches!(v, "auto" | "low"))
        {
            return Err((
                400,
                "invalid_request_error",
                "moderation 只能是 auto 或 low".to_string(),
            ));
        }
        if self
            .input_fidelity
            .as_deref()
            .is_some_and(|v| !matches!(v, "high" | "low"))
        {
            return Err((
                400,
                "invalid_request_error",
                "input_fidelity 只能是 high 或 low".to_string(),
            ));
        }
        if !matches!(
            self.requested_size().as_str(),
            "auto" | "1024x1024" | "1536x1024" | "1024x1536"
        ) {
            return Err((
                400,
                "invalid_request_error",
                "size 只能是 auto、1024x1024、1536x1024 或 1024x1536".to_string(),
            ));
        }
        if self
            .images
            .iter()
            .any(|item| item.file_id.as_ref().is_some_and(|v| !v.is_empty()))
            || self
                .mask
                .as_ref()
                .and_then(|item| item.file_id.as_ref())
                .is_some_and(|v| !v.is_empty())
        {
            return Err((
                400,
                "unsupported_parameter",
                "本地桌面端暂不提供 Files API，请使用 image_url 传入图片 URL 或 data URL"
                    .to_string(),
            ));
        }
        if endpoint == ImageEndpoint::Edit && self.reference_urls().is_empty() {
            return Err((
                400,
                "invalid_request_error",
                "图片编辑接口需要至少一个 images[].image_url".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct ImageGenContext {
    pub platform: String,
    pub model_display_name: String,
    pub flow: crate::types::FlowDef,
    pub account_id: String,
    pub profile_dir: String,
    pub account_cookies: String,
    pub request_id: String,
    pub browser_session_id: String,
    pub model_id: String,
    pub prompt: String,
    pub image_aspect_ratio: String,
    pub ref_image_local: String,
    pub ref_image_locals: Vec<String>,
    pub workspace_dir: std::path::PathBuf,
    pub initial_context: serde_json::Value,
    pub output_format: String,
    pub quality: String,
    pub size: String,
    pub output_compression: Option<u32>,
    pub background: Option<String>,
}
