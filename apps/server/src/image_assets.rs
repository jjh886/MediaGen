use base64::Engine;
use uuid::Uuid;

use crate::net_client::http_client;

pub(crate) async fn collect_b64_images(
    result: &serde_json::Value,
    proxy_url: &str,
) -> Result<Vec<String>, String> {
    let mut images = Vec::new();
    if let Some(items) = result.get("images").and_then(serde_json::Value::as_array) {
        for item in items {
            if let Some(url) = item.get("url").and_then(serde_json::Value::as_str) {
                images.push(image_url_to_b64(url, proxy_url).await?);
            }
        }
    }
    if images.is_empty() {
        if let Some(url) = result
            .get("download_url")
            .or_else(|| result.get("url"))
            .and_then(serde_json::Value::as_str)
        {
            images.push(image_url_to_b64(url, proxy_url).await?);
        }
    }
    if images.is_empty() {
        return Err("插件没有返回可下载图片".to_string());
    }
    Ok(images)
}

pub(crate) async fn fetch_ref_images_as_data_urls(urls: &[String], proxy_url: &str) -> Vec<String> {
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

pub(crate) async fn fetch_ref_images_as_local_files(
    urls: &[String],
    proxy_url: &str,
) -> Vec<String> {
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

pub(crate) async fn cleanup_temp_files(paths: &[String]) {
    for path in paths {
        let _ = tokio::fs::remove_file(path).await;
    }
}

pub(crate) fn cleanup_stale_temp_files() {
    cleanup_temp_files_in(&std::env::temp_dir());
}

fn cleanup_temp_files_in(root: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let is_reference = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("mediagen_ref_"));
        if is_reference {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cleanup_temp_files_in;

    #[test]
    fn stale_cleanup_only_removes_mediagen_reference_files() {
        let root = std::env::temp_dir().join("mediagen-stale-image-cleanup-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let stale = root.join("mediagen_ref_stale.png");
        let unrelated = root.join("keep.png");
        std::fs::write(&stale, b"image").unwrap();
        std::fs::write(&unrelated, b"image").unwrap();

        cleanup_temp_files_in(&root);

        assert!(!stale.exists());
        assert!(unrelated.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}

pub(crate) async fn fetch_optional_data_url(url: Option<&str>, proxy_url: &str) -> Option<String> {
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
