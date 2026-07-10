use actix_web::HttpResponse;
use chrono::Utc;
use mediagen_core::PluginManifest;
use serde_json::{json, Value};

use crate::image_request::{ImageEndpoint, OpenAiImageRequest};
use crate::store::TaskRecord;

pub(crate) fn new_image_task(
    task_id: &str,
    manifest: &PluginManifest,
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
        status: "queued".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: Utc::now().to_rfc3339(),
        completed_at: None,
    }
}

pub(crate) fn openai_error(status: u16, code: &'static str, message: &str) -> HttpResponse {
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

pub(crate) fn openai_image_stream(body: Value, endpoint: ImageEndpoint) -> HttpResponse {
    let event = match endpoint {
        ImageEndpoint::Generation => "image_generation.completed",
        ImageEndpoint::Edit => "image_edit.completed",
        ImageEndpoint::Variation => "image_variation.completed",
    };
    HttpResponse::Ok()
        .insert_header(("content-type", "text/event-stream; charset=utf-8"))
        .insert_header(("cache-control", "no-cache"))
        .body(format!(
            "event: {event}\ndata: {body}\n\nevent: done\ndata: [DONE]\n\n"
        ))
}
