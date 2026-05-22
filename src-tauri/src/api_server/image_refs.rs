/// 并发下载参考图并转 `data:<mime>;base64,<b64>` 字符串。
/// 目标：让 API 模式插件在浏览器里零 CORS 地读取参考图 bytes。
/// 单张下载失败跳过（不塞空条目），JS 侧会按原 URL 退回尝试 fetch。
pub(crate) async fn fetch_ref_images_as_data_urls(urls: &[String]) -> Vec<String> {
    use base64::Engine as _;
    if urls.is_empty() {
        return Vec::new();
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut out = Vec::with_capacity(urls.len());
    for url in urls {
        if url.starts_with("data:image/") {
            out.push(url.clone());
            continue;
        }
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let mime = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.split(';').next().map(|x| x.trim().to_string()))
                    .unwrap_or_else(|| "image/png".to_string());
                match resp.bytes().await {
                    Ok(bytes) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        out.push(format!("data:{mime};base64,{b64}"));
                    }
                    Err(e) => {
                        tracing::warn!(url, error = %e, "参考图 body 读取失败");
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!(url, status = %resp.status(), "参考图 HTTP 非 2xx");
            }
            Err(e) => {
                tracing::warn!(url, error = %e, "参考图请求失败");
            }
        }
    }
    out
}
