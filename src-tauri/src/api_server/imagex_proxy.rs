//! VolcEngine ImageX TOS 上传代理
//!
//! 背景：浏览器从豆包页面 fetch 到 `tos-*.snssdk.com` 的跨域 POST 经常被 CORS 拒绝
//! （`TypeError: Failed to fetch`），尤其是字节按区域路由到 `tos-hl-x`/`tos-cn-*` 等
//! 非常用节点时。而 putBlob 这一步**不需要签名**（HAR 抓包确认仅需 Content-CRC32 +
//! X-Storage-U + 固定 Origin/Referer），非常适合让 Rust reqwest 代理。
//!
//! 接口：`POST /api/internal/imagex_put_blob?host=...&uri=...&crc32=...&user_id=...`
//!   body：图片原始字节（Content-Type: application/octet-stream）
//!   返回：目标服务 JSON 原样回传

use actix_web::{web, HttpResponse};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct PutBlobQuery {
    pub host: String,
    pub uri: String,
    pub crc32: String,
    #[serde(default)]
    pub user_id: String,
}

pub(crate) async fn imagex_put_blob(
    query: web::Query<PutBlobQuery>,
    body: web::Bytes,
) -> HttpResponse {
    if query.host.is_empty() || query.uri.is_empty() {
        return HttpResponse::BadRequest().body("host and uri required");
    }
    let target = format!("https://{}/upload/v1/{}", query.host, query.uri);
    tracing::info!(
        host = %query.host,
        uri = %query.uri,
        crc32 = %query.crc32,
        user_id = %query.user_id,
        bytes = body.len(),
        "imagex_put_blob 代理上传"
    );

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("reqwest client build failed: {e}"))
        }
    };

    let resp_result = client
        .post(&target)
        .header("Content-Type", "application/octet-stream")
        .header("Content-CRC32", &query.crc32)
        .header("Content-Disposition", "attachment; filename=\"undefined\"")
        .header("X-Storage-U", &query.user_id)
        .header("Origin", "https://www.doubao.com")
        .header("Referer", "https://www.doubao.com/")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36",
        )
        .body(body.to_vec())
        .send()
        .await;

    let resp = match resp_result {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target = %target, error = %e, "imagex_put_blob 上传失败");
            return HttpResponse::BadGateway().body(format!("target request failed: {e}"));
        }
    };

    let status = resp.status();
    let target_body = resp.bytes().await.unwrap_or_default();
    let content_type = "application/json";
    tracing::info!(
        status = %status,
        bytes_resp = target_body.len(),
        "imagex_put_blob 上传完成"
    );

    HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status.as_u16())
            .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
    )
    .content_type(content_type)
    .body(target_body.to_vec())
}
