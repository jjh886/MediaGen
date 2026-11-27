use futures_util::StreamExt;

/// 大 JSON 请求先落盘，避免同时在内存里保留原始请求和解析后的附件。
pub(crate) async fn read_json<T: serde::de::DeserializeOwned + Send + 'static>(
    mut payload: impl futures_util::Stream<Item = Result<bytes::Bytes, actix_web::error::PayloadError>>
        + Unpin,
) -> Result<T, (u16, &'static str, String)> {
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};
    let io_error = |_| (500, "server_error", "读取临时请求文件失败".to_string());
    let mut buffer = Vec::new();
    let mut spool: Option<tokio::fs::File> = None;
    while let Some(chunk) = payload.next().await {
        let chunk = chunk.map_err(|_| (400, "invalid_body", "请求体读取失败".into()))?;
        if spool.is_none() && buffer.len().saturating_add(chunk.len()) > 1024 * 1024 {
            let mut file = tokio::fs::File::from_std(tempfile::tempfile().map_err(io_error)?);
            file.write_all(&buffer).await.map_err(io_error)?;
            buffer = Vec::new();
            spool = Some(file);
        }
        if let Some(file) = &mut spool {
            file.write_all(&chunk).await.map_err(io_error)?;
        } else {
            buffer.extend_from_slice(&chunk);
        }
    }
    let parsed = if let Some(mut file) = spool {
        file.rewind().await.map_err(io_error)?;
        let file = file.into_std().await;
        tokio::task::spawn_blocking(move || serde_json::from_reader(std::io::BufReader::new(file)))
            .await
            .map_err(|_| (500, "server_error", "解析请求失败".to_string()))?
    } else {
        serde_json::from_slice(&buffer)
    };
    parsed.map_err(|_| (400, "invalid_json", "请求体不是有效 JSON".into()))
}

use base64::prelude::{Engine as _, BASE64_STANDARD};

/// 将 multipart 中的图片转为插件可直接消费的 data URL。
pub(crate) fn image_data_url(
    content_type: Option<&str>,
    filename: Option<&str>,
    bytes: &[u8],
) -> String {
    let mime = image_mime(content_type, filename, bytes);
    format!("data:{mime};base64,{}", BASE64_STANDARD.encode(bytes))
}

pub(crate) fn image_mime(
    content_type: Option<&str>,
    filename: Option<&str>,
    bytes: &[u8],
) -> &'static str {
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
    fn detects_mime_from_headers_content_and_filename() {
        assert_eq!(image_mime(Some("image/jpg"), None, b""), "image/jpeg");
        assert_eq!(
            image_mime(None, None, b"\x89PNG\r\n\x1a\nrest"),
            "image/png"
        );
        assert_eq!(image_mime(None, None, &[0xff, 0xd8, 0xff]), "image/jpeg");
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
        assert_eq!(
            image_data_url(Some("image/png"), Some("ignored.jpg"), b"abc"),
            "data:image/png;base64,YWJj"
        );
    }
}
