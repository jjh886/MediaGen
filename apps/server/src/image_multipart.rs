use actix_multipart::{Field, Multipart};
use actix_web::http::header;
use actix_web::{web, HttpRequest};
use base64::prelude::{Engine as _, BASE64_STANDARD};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use serde_json::Value;

use crate::image_request::{OpenAiImageRef, OpenAiImageRequest};

const MAX_BODY_BYTES: usize = 50 * 1024 * 1024;

pub type ParseError = (u16, &'static str, String);

pub async fn parse_image_request(
    req: HttpRequest,
    payload: web::Payload,
) -> Result<OpenAiImageRequest, ParseError> {
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if content_type.starts_with("multipart/form-data") {
        return parse_multipart(Multipart::new(req.headers(), payload)).await;
    }
    if content_type.is_empty() || content_type.starts_with("application/json") {
        return parse_json(payload).await;
    }
    Err((
        415,
        "unsupported_media_type",
        "图片接口只支持 application/json 或 multipart/form-data".to_string(),
    ))
}

async fn parse_json(payload: web::Payload) -> Result<OpenAiImageRequest, ParseError> {
    let bytes = read_payload(payload).await?;
    serde_json::from_slice(&bytes).map_err(|err| {
        (
            400,
            "invalid_request_error",
            format!("JSON 请求体解析失败: {err}"),
        )
    })
}

async fn parse_multipart(mut multipart: Multipart) -> Result<OpenAiImageRequest, ParseError> {
    let mut request = OpenAiImageRequest::empty();
    let mut total = 0usize;
    while let Some(item) = multipart.next().await {
        let mut field = item.map_err(|err| {
            (
                400,
                "invalid_request_error",
                format!("multipart 字段解析失败: {err}"),
            )
        })?;
        let name = field_name(&field);
        let filename = field_filename(&field);
        let content_type = field_content_type(&field);
        let bytes = read_field(&mut field, &mut total).await?;

        if is_image_field(&name) || name == "mask" {
            let image_ref = OpenAiImageRef {
                file_id: None,
                image_url: Some(to_data_url(
                    content_type.as_deref(),
                    filename.as_deref(),
                    &bytes,
                )),
            };
            if name == "mask" {
                request.mask = Some(image_ref);
            } else {
                request.images.push(image_ref);
            }
            continue;
        }
        apply_text_field(&mut request, &name, &String::from_utf8_lossy(&bytes));
    }
    Ok(request)
}

async fn read_payload(mut payload: web::Payload) -> Result<Bytes, ParseError> {
    let mut body = BytesMut::new();
    while let Some(chunk) = payload.next().await {
        let chunk = chunk.map_err(|err| {
            (
                400,
                "invalid_request_error",
                format!("请求体读取失败: {err}"),
            )
        })?;
        ensure_size(body.len() + chunk.len())?;
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

async fn read_field(field: &mut Field, total: &mut usize) -> Result<Bytes, ParseError> {
    let mut body = BytesMut::new();
    while let Some(chunk) = field.next().await {
        let chunk = chunk.map_err(|err| {
            (
                400,
                "invalid_request_error",
                format!("multipart 内容读取失败: {err}"),
            )
        })?;
        *total += chunk.len();
        ensure_size(*total)?;
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

fn apply_text_field(request: &mut OpenAiImageRequest, name: &str, value: &str) {
    let value = value.trim();
    match name {
        "model" => request.model = value.to_string(),
        "prompt" => request.prompt = Some(Value::String(value.to_string())),
        "n" => request.n = value.parse().ok(),
        "size" => request.size = Some(value.to_string()),
        "quality" => request.quality = Some(value.to_string()),
        "response_format" => request.response_format = Some(value.to_string()),
        "output_format" => request.output_format = Some(value.to_string()),
        "background" => request.background = Some(value.to_string()),
        "stream" => request.stream = Some(matches!(value, "true" | "1")),
        "partial_images" => request.partial_images = value.parse().ok(),
        "output_compression" => request.output_compression = value.parse().ok(),
        "moderation" => request.moderation = Some(value.to_string()),
        "input_fidelity" => request.input_fidelity = Some(value.to_string()),
        "user" => request.user = Some(value.to_string()),
        "image_aspect_ratio" => request.image_aspect_ratio = Some(value.to_string()),
        _ => {}
    }
}

fn field_name(field: &Field) -> String {
    field
        .content_disposition()
        .and_then(|value| value.get_name())
        .unwrap_or("")
        .to_string()
}

fn field_filename(field: &Field) -> Option<String> {
    field
        .content_disposition()
        .and_then(|value| value.get_filename())
        .map(ToString::to_string)
}

fn field_content_type(field: &Field) -> Option<String> {
    field
        .content_type()
        .map(|value| value.essence_str().to_string())
}

fn is_image_field(name: &str) -> bool {
    matches!(name, "image" | "image[]") || name.starts_with("image[")
}

fn ensure_size(size: usize) -> Result<(), ParseError> {
    if size <= MAX_BODY_BYTES {
        Ok(())
    } else {
        Err((
            413,
            "request_too_large",
            "图片请求体不能超过 50MB".to_string(),
        ))
    }
}

fn to_data_url(content_type: Option<&str>, filename: Option<&str>, bytes: &[u8]) -> String {
    let mime = image_mime(content_type, filename, bytes);
    format!("data:{mime};base64,{}", BASE64_STANDARD.encode(bytes))
}

fn image_mime(content_type: Option<&str>, filename: Option<&str>, bytes: &[u8]) -> &'static str {
    if let Some(mime) = content_type.and_then(normalize_image_mime) {
        return mime;
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    filename.and_then(filename_mime).unwrap_or("image/png")
}

fn normalize_image_mime(value: &str) -> Option<&'static str> {
    match value {
        "image/png" => Some("image/png"),
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/webp" => Some("image/webp"),
        _ => None,
    }
}

fn filename_mime(filename: &str) -> Option<&'static str> {
    match std::path::Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_fields_update_request_values() {
        let mut request = OpenAiImageRequest::empty();
        apply_text_field(&mut request, "model", " gpt-image-1 ");
        apply_text_field(&mut request, "prompt", " draw ");
        apply_text_field(&mut request, "n", "2");
        apply_text_field(&mut request, "stream", "1");
        apply_text_field(&mut request, "partial_images", "3");
        apply_text_field(&mut request, "output_compression", "80");
        apply_text_field(&mut request, "image_aspect_ratio", "16:9");
        apply_text_field(&mut request, "unknown", "ignored");

        assert_eq!(request.model, "gpt-image-1");
        assert_eq!(
            request
                .prompt
                .and_then(|value| value.as_str().map(str::to_string)),
            Some("draw".to_string())
        );
        assert_eq!(request.n, Some(2));
        assert_eq!(request.stream, Some(true));
        assert_eq!(request.partial_images, Some(3));
        assert_eq!(request.output_compression, Some(80));
        assert_eq!(request.image_aspect_ratio.as_deref(), Some("16:9"));
    }

    #[test]
    fn detects_image_fields_and_size_limits() {
        assert!(is_image_field("image"));
        assert!(is_image_field("image[]"));
        assert!(is_image_field("image[0]"));
        assert!(!is_image_field("mask"));
        assert!(ensure_size(MAX_BODY_BYTES).is_ok());
        assert_eq!(ensure_size(MAX_BODY_BYTES + 1).unwrap_err().0, 413);
    }

    #[test]
    fn infers_safe_image_mime_types() {
        assert_eq!(image_mime(Some("image/jpg"), None, b""), "image/jpeg");
        assert_eq!(
            image_mime(None, None, b"\x89PNG\r\n\x1a\nrest"),
            "image/png"
        );
        assert_eq!(
            image_mime(None, None, &[0xff, 0xd8, 0xff, 0x00]),
            "image/jpeg"
        );
        assert_eq!(
            image_mime(None, Some("picture.webp"), b"data"),
            "image/webp"
        );
        assert_eq!(
            image_mime(Some("text/plain"), Some("bad.txt"), b"data"),
            "image/png"
        );
    }

    #[test]
    fn converts_binary_image_to_data_url() {
        let data_url = to_data_url(Some("image/png"), Some("ignored.jpg"), b"abc");
        assert_eq!(data_url, "data:image/png;base64,YWJj");
    }
}
