use base64::prelude::{Engine as _, BASE64_STANDARD};

use super::ImageGenContext;
use crate::error::AppError;

/// 把 reqwest / 标准 Error 的 source 链展平成一行，方便定位真实失败原因。
fn err_chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        out.push_str(" | caused by: ");
        out.push_str(&s.to_string());
        src = s.source();
    }
    out
}

fn read_local_bytes(path: &std::path::Path) -> Option<bytes::Bytes> {
    match std::fs::read(path) {
        Ok(v) if !v.is_empty() => Some(bytes::Bytes::from(v)),
        Ok(_) => {
            tracing::warn!(path = %path.display(), "本地图片为空，回退到 URL 下载");
            None
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "读取本地图片失败，回退到 URL 下载");
            None
        }
    }
}

pub(crate) async fn download_image_with_retry(
    client: &reqwest::Client,
    url: &str,
) -> Result<bytes::Bytes, String> {
    if let Some(bytes) = decode_data_url(url) {
        return bytes;
    }
    for attempt in 0..3 {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => return Ok(bytes),
                Err(e) => tracing::warn!(attempt, error = %err_chain(&e), "读取图片数据失败"),
            },
            Ok(resp) => tracing::warn!(attempt, status = %resp.status(), url, "下载图片失败"),
            Err(e) => tracing::warn!(attempt, error = %err_chain(&e), "下载图片请求失败"),
        }
        if attempt < 2 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }
    Err(format!("下载图片失败（3 次重试后放弃）: {url}"))
}

fn decode_data_url(url: &str) -> Option<Result<bytes::Bytes, String>> {
    let (_, encoded) = url.strip_prefix("data:image/")?.split_once(";base64,")?;
    Some(
        BASE64_STANDARD
            .decode(encoded)
            .map(bytes::Bytes::from)
            .map_err(|e| format!("data URL 图片解码失败: {e}")),
    )
}

/// 单张图片处理结果。`b64_json` 与 OpenAI Image API 字段保持一致。
pub(crate) struct ImageResult {
    pub b64_json: String,
    pub width: u64,
    pub height: u64,
}

async fn consume_polled_image(
    i: usize,
    raw_url: &str,
    w: u64,
    h: u64,
    local_downloads: &[std::path::PathBuf],
    ctx: &ImageGenContext,
    client: &reqwest::Client,
    ts: &str,
) -> Option<ImageResult> {
    if raw_url.is_empty() {
        return None;
    }

    let bytes = match local_downloads.get(i).and_then(|p| read_local_bytes(p)) {
        Some(b) => {
            tracing::info!(index = i, path = %local_downloads[i].display(), size = b.len(), "使用插件已下载的本地图片");
            b
        }
        None => match download_image_with_retry(client, raw_url).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(index = i, error = %e, "跳过该图");
                return None;
            }
        },
    };

    save_workspace_copy(i, &bytes, ctx, ts);

    Some(ImageResult {
        b64_json: BASE64_STANDARD.encode(&bytes),
        width: w,
        height: h,
    })
}

fn save_workspace_copy(i: usize, bytes: &[u8], ctx: &ImageGenContext, ts: &str) {
    let ext = match ctx.output_format.as_str() {
        "jpeg" => "jpg",
        "webp" => "webp",
        _ => "png",
    };
    let path = ctx.workspace_dir.join(format!("{ts}_{i}.{ext}"));
    if let Err(e) = std::fs::write(&path, bytes) {
        tracing::warn!(error = %e, "保存图片副本失败");
    } else {
        tracing::info!(path = %path.display(), size = bytes.len(), "已保存图片副本到工作区");
    }
}

/// 在 flow step results 里找到已经下载到本地的文件路径。
pub(crate) fn collect_local_downloads(
    results: &[crate::types::StepResult],
) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    for r in results {
        let Some(path_str) = r.data.get("path").and_then(|v| v.as_str()) else {
            continue;
        };
        if r.data.get("size").is_none() || path_str.is_empty() {
            continue;
        }
        let p = std::path::PathBuf::from(path_str);
        if p.is_file() {
            paths.push(p);
        }
    }
    paths
}

/// 处理插件 poll 结果：读取图片字节并直接编码为 OpenAI 风格的 base64。
pub(crate) async fn process_poll_images(
    poll_data: Option<&serde_json::Value>,
    ctx: &ImageGenContext,
    results: &[crate::types::StepResult],
) -> Vec<ImageResult> {
    let mut image_results = Vec::new();
    let images = poll_data.and_then(|d| d["images"].as_array()).or_else(|| {
        results
            .iter()
            .rev()
            .find(|r| r.data.get("status").and_then(|v| v.as_str()) == Some("done"))
            .and_then(|r| r.data["images"].as_array())
    });

    let single_download = poll_data.and_then(single_download_from_value).or_else(|| {
        results
            .iter()
            .rev()
            .find_map(|r| single_download_from_value(&r.data))
    });

    let total = images
        .map(|arr| arr.len())
        .filter(|len| *len > 0)
        .unwrap_or_else(|| if single_download.is_some() { 1 } else { 0 });
    if total == 0 {
        tracing::error!("poll 返回 done 但 images / download_url 都为空");
        return image_results;
    }

    let client = reqwest::Client::new();
    let ts = chrono::Local::now().format("%H%M%S").to_string();
    let local_downloads = collect_local_downloads(results);

    if let Some(arr) = images.filter(|arr| !arr.is_empty()) {
        for (i, img) in arr.iter().enumerate() {
            let raw_url = img["url"].as_str().unwrap_or("");
            let w = img["width"].as_u64().unwrap_or(0);
            let h = img["height"].as_u64().unwrap_or(0);
            if let Some(result) =
                consume_polled_image(i, raw_url, w, h, &local_downloads, ctx, &client, &ts).await
            {
                image_results.push(result);
            }
        }
    } else if let Some((raw_url, w, h)) = single_download {
        tracing::info!(url = %raw_url, "poll 结果为单文件 download_url，按 1 张图处理");
        if let Some(result) =
            consume_polled_image(0, &raw_url, w, h, &local_downloads, ctx, &client, &ts).await
        {
            image_results.push(result);
        }
    }

    image_results
}

fn single_download_from_value(value: &serde_json::Value) -> Option<(String, u64, u64)> {
    let url = value["download_url"].as_str()?.trim();
    if url.is_empty() {
        return None;
    }
    Some((
        url.to_string(),
        value["width"].as_u64().unwrap_or(0),
        value["height"].as_u64().unwrap_or(0),
    ))
}

/// 下载参考图 URL 到本地临时文件，供浏览器文件输入类插件使用。
pub(crate) async fn download_ref_image(url: &str) -> Result<String, AppError> {
    let bytes = if let Some(decoded) = decode_data_url(url) {
        decoded.map_err(AppError::PlatformError)?
    } else {
        let resp = reqwest::get(url)
            .await
            .map_err(|e| AppError::PlatformError(format!("参考图下载失败: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::PlatformError(format!(
                "参考图下载失败: HTTP {}",
                resp.status()
            )));
        }
        resp.bytes()
            .await
            .map_err(|e| AppError::PlatformError(format!("参考图读取失败: {e}")))?
    };
    let path = std::env::temp_dir().join(format!("frame64_ref_{}.png", uuid::Uuid::new_v4()));
    tokio::fs::write(&path, &bytes).await?;
    Ok(path.to_string_lossy().to_string())
}

/// 并发下载多张参考图。单张失败只记录日志，不影响其它图片。
pub(crate) async fn download_ref_images(urls: &[String]) -> Vec<String> {
    use futures::future::join_all;
    let futs = urls.iter().enumerate().map(|(i, url)| {
        let url = url.clone();
        async move {
            match download_ref_image(&url).await {
                Ok(path) => Some((i, path)),
                Err(e) => {
                    tracing::warn!(index = i, url = %url, error = %e, "参考图下载失败，跳过");
                    None
                }
            }
        }
    });
    let mut results: Vec<(usize, String)> = join_all(futs).await.into_iter().flatten().collect();
    results.sort_by_key(|(i, _)| *i);
    results.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_local_downloads_filters_invalid_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        std::fs::write(&path, b"ok").unwrap();
        let results = vec![
            crate::types::StepResult {
                success: true,
                data: serde_json::json!({ "path": path, "size": 2 }),
                error: None,
            },
            crate::types::StepResult {
                success: true,
                data: serde_json::json!({ "path": "/tmp/frame64-not-exist.png", "size": 1 }),
                error: None,
            },
        ];

        let paths = collect_local_downloads(&results);
        assert_eq!(paths.len(), 1);
    }
}
