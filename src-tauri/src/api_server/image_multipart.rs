use actix_multipart::{Field, Multipart};
use actix_web::http::header;
use actix_web::{web, HttpRequest};
use base64::prelude::{Engine as _, BASE64_STANDARD};
use bytes::{Bytes, BytesMut};
use futures::StreamExt;

use super::{OpenAiImageRef, OpenAiImageRequest};

const MAX_EDIT_BODY_BYTES: usize = 50 * 1024 * 1024;

pub(crate) type ParseError = (u16, &'static str, String);

/// 解析 `/v1/images/edits` 请求。
/// OpenAI SDK 会用 multipart 上传图片；本地旧调用仍可继续传 JSON。
pub(crate) async fn parse_edit_request(
    req: HttpRequest,
    payload: web::Payload,
) -> Result<OpenAiImageRequest, ParseError> {
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
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
        "图片编辑接口只支持 application/json 或 multipart/form-data".to_string(),
    ))
}

async fn parse_json(payload: web::Payload) -> Result<OpenAiImageRequest, ParseError> {
    let bytes = read_payload(payload).await?;
    serde_json::from_slice(&bytes).map_err(|e| {
        (
            400,
            "invalid_request_error",
            format!("JSON 请求体解析失败: {e}"),
        )
    })
}

async fn parse_multipart(mut multipart: Multipart) -> Result<OpenAiImageRequest, ParseError> {
    let mut req = empty_request();
    let mut total = 0usize;

    while let Some(item) = multipart.next().await {
        let mut field = item.map_err(|e| {
            (
                400,
                "invalid_request_error",
                format!("multipart 字段解析失败: {e}"),
            )
        })?;
        let name = field_name(&field);
        let filename = field_filename(&field);
        let content_type = field_content_type(&field);
        let bytes = read_field(&mut field, &mut total).await?;

        if is_image_field(&name) {
            let data_url = to_data_url(content_type.as_deref(), filename.as_deref(), &bytes);
            req.images.push(OpenAiImageRef {
                file_id: None,
                image_url: Some(data_url),
            });
            continue;
        }
        if name == "mask" {
            let data_url = to_data_url(content_type.as_deref(), filename.as_deref(), &bytes);
            req.mask = Some(OpenAiImageRef {
                file_id: None,
                image_url: Some(data_url),
            });
            continue;
        }

        let value = String::from_utf8_lossy(&bytes).trim().to_string();
        apply_text_field(&mut req, &name, &value);
    }

    Ok(req)
}

fn empty_request() -> OpenAiImageRequest {
    OpenAiImageRequest {
        model: String::new(),
        prompt: None,
        n: None,
        size: None,
        quality: None,
        response_format: None,
        output_format: None,
        background: None,
        stream: None,
        partial_images: None,
        output_compression: None,
        moderation: None,
        input_fidelity: None,
        user: None,
        images: Vec::new(),
        mask: None,
        image_aspect_ratio: None,
    }
}

async fn read_payload(mut payload: web::Payload) -> Result<Bytes, ParseError> {
    let mut body = BytesMut::new();
    while let Some(chunk) = payload.next().await {
        let chunk =
            chunk.map_err(|e| (400, "invalid_request_error", format!("请求体读取失败: {e}")))?;
        ensure_size(body.len() + chunk.len())?;
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

async fn read_field(field: &mut Field, total: &mut usize) -> Result<Bytes, ParseError> {
    let mut body = BytesMut::new();
    while let Some(chunk) = field.next().await {
        let chunk = chunk.map_err(|e| {
            (
                400,
                "invalid_request_error",
                format!("multipart 内容读取失败: {e}"),
            )
        })?;
        *total += chunk.len();
        ensure_size(*total)?;
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

fn ensure_size(size: usize) -> Result<(), ParseError> {
    if size <= MAX_EDIT_BODY_BYTES {
        return Ok(());
    }
    Err((
        413,
        "request_too_large",
        "图片编辑请求体不能超过 50MB".to_string(),
    ))
}

fn field_name(field: &Field) -> String {
    field
        .content_disposition()
        .and_then(|d| d.get_name())
        .unwrap_or("")
        .to_string()
}

fn field_filename(field: &Field) -> Option<String> {
    field
        .content_disposition()
        .and_then(|d| d.get_filename())
        .map(ToString::to_string)
}

fn field_content_type(field: &Field) -> Option<String> {
    field.content_type().map(|m| m.essence_str().to_string())
}

fn is_image_field(name: &str) -> bool {
    matches!(name, "image" | "image[]") || name.starts_with("image[")
}

fn apply_text_field(req: &mut OpenAiImageRequest, name: &str, value: &str) {
    match name {
        "model" => req.model = value.to_string(),
        "prompt" => req.prompt = Some(value.to_string()),
        "n" => req.n = value.parse().ok(),
        "size" => req.size = Some(value.to_string()),
        "quality" => req.quality = Some(value.to_string()),
        "response_format" => req.response_format = Some(value.to_string()),
        "output_format" => req.output_format = Some(value.to_string()),
        "background" => req.background = Some(value.to_string()),
        "stream" => req.stream = Some(matches!(value, "true" | "1")),
        "partial_images" => req.partial_images = value.parse().ok(),
        "output_compression" => req.output_compression = value.parse().ok(),
        "moderation" => req.moderation = Some(value.to_string()),
        "input_fidelity" => req.input_fidelity = Some(value.to_string()),
        "user" => req.user = Some(value.to_string()),
        "image_aspect_ratio" => req.image_aspect_ratio = Some(value.to_string()),
        // OpenAI 新参数先忽略，不影响 SDK 兼容；真实能力由插件流程决定。
        _ => {}
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
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return "image/webp";
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
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())?;
    match ext.as_str() {
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
    fn image_field_accepts_openai_array_names() {
        assert!(is_image_field("image"));
        assert!(is_image_field("image[]"));
        assert!(is_image_field("image[0]"));
        assert!(!is_image_field("images"));
    }

    #[test]
    fn text_fields_fill_openai_request() {
        let mut req = empty_request();
        apply_text_field(&mut req, "model", "gpt-image-1");
        apply_text_field(&mut req, "prompt", "hello");
        apply_text_field(&mut req, "n", "2");
        apply_text_field(&mut req, "stream", "true");
        apply_text_field(&mut req, "partial_images", "2");

        assert_eq!(req.model, "gpt-image-1");
        assert_eq!(req.prompt.as_deref(), Some("hello"));
        assert_eq!(req.n, Some(2));
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.partial_images, Some(2));
    }

    #[test]
    fn data_url_infers_png_from_magic_bytes() {
        let data_url = to_data_url(None, Some("image.bin"), b"\x89PNG\r\n\x1a\nabc");

        assert!(data_url.starts_with("data:image/png;base64,"));
    }
}
