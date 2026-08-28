use actix_multipart::{Field, Multipart};
use actix_web::http::header;
use actix_web::{web, HttpRequest};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use serde_json::Value;

use crate::image_multipart::{field_filename, field_name};
use crate::image_request::OpenAiImageRef;
use crate::media_execution::ApiError;
use crate::media_multipart::image_data_url;
use crate::video_request::OpenAiVideoRequest;

const MAX_BODY_BYTES: usize = 50 * 1024 * 1024;

pub(crate) async fn parse_video_request(
    request: HttpRequest,
    payload: web::Payload,
) -> Result<OpenAiVideoRequest, ApiError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if content_type.starts_with("multipart/form-data") {
        return parse_multipart(Multipart::new(request.headers(), payload)).await;
    }
    if content_type.is_empty() || content_type.starts_with("application/json") {
        return parse_json(payload).await;
    }
    Err((
        415,
        "unsupported_media_type",
        "视频接口只支持 application/json 或 multipart/form-data".to_string(),
    ))
}

async fn parse_json(payload: web::Payload) -> Result<OpenAiVideoRequest, ApiError> {
    let bytes = read_payload(payload).await?;
    serde_json::from_slice(&bytes).map_err(|error| {
        (
            400,
            "invalid_request_error",
            format!("JSON 请求体解析失败: {error}"),
        )
    })
}

async fn parse_multipart(mut multipart: Multipart) -> Result<OpenAiVideoRequest, ApiError> {
    let mut request = OpenAiVideoRequest::empty();
    let mut total = 0usize;
    while let Some(item) = multipart.next().await {
        let mut field = item.map_err(multipart_error)?;
        let name = field_name(&field);
        let filename = field_filename(&field);
        let content_type = field
            .content_type()
            .map(|value| value.essence_str().to_string());
        let bytes = read_field(&mut field, &mut total).await?;
        if matches!(name.as_str(), "input_reference" | "input_references[]") {
            let reference = OpenAiImageRef {
                file_id: None,
                image_url: Some(image_data_url(
                    content_type.as_deref(),
                    filename.as_deref(),
                    &bytes,
                )),
            };
            if name == "input_reference" && request.input_reference.is_none() {
                request.input_reference = Some(reference);
            } else {
                request.input_references.push(reference);
            }
            continue;
        }
        apply_text_field(&mut request, &name, &String::from_utf8_lossy(&bytes));
    }
    Ok(request)
}

fn apply_text_field(request: &mut OpenAiVideoRequest, name: &str, value: &str) {
    let value = value.trim();
    match name {
        "model" => request.model = value.to_string(),
        "prompt" => request.prompt = value.to_string(),
        "seconds" => request.seconds = Some(Value::String(value.to_string())),
        "size" => request.size = Some(value.to_string()),
        "quality" => request.quality = Some(value.to_string()),
        "user" => request.user = Some(value.to_string()),
        "input_reference" if value.starts_with("file-") => {
            request.input_reference = Some(OpenAiImageRef {
                file_id: Some(value.to_string()),
                image_url: None,
            });
        }
        "input_reference" => {
            request.input_reference = Some(OpenAiImageRef {
                file_id: None,
                image_url: Some(value.to_string()),
            });
        }
        _ => {
            request
                .extra
                .insert(name.to_string(), Value::String(value.to_string()));
        }
    }
}

async fn read_payload(mut payload: web::Payload) -> Result<Bytes, ApiError> {
    let mut body = BytesMut::new();
    while let Some(chunk) = payload.next().await {
        let chunk = chunk.map_err(multipart_error)?;
        ensure_size(body.len() + chunk.len())?;
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

async fn read_field(field: &mut Field, total: &mut usize) -> Result<Bytes, ApiError> {
    let mut body = BytesMut::new();
    while let Some(chunk) = field.next().await {
        let chunk = chunk.map_err(multipart_error)?;
        *total += chunk.len();
        ensure_size(*total)?;
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

fn ensure_size(size: usize) -> Result<(), ApiError> {
    if size <= MAX_BODY_BYTES {
        Ok(())
    } else {
        Err((
            413,
            "request_too_large",
            "视频请求体不能超过 50MB".to_string(),
        ))
    }
}

fn multipart_error(error: impl std::fmt::Display) -> ApiError {
    (
        400,
        "invalid_request_error",
        format!("multipart 内容解析失败: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_standard_video_fields() {
        let mut request = OpenAiVideoRequest::empty();
        apply_text_field(&mut request, "model", "seedance");
        apply_text_field(&mut request, "prompt", "move");
        apply_text_field(&mut request, "seconds", "8");
        apply_text_field(&mut request, "input_reference", "file-123");

        assert_eq!(request.model, "seedance");
        assert_eq!(request.seconds_text().unwrap(), "8");
        assert_eq!(
            request.input_reference.and_then(|item| item.file_id),
            Some("file-123".to_string())
        );
    }
}
