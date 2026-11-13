use super::{error, execute};
use crate::ServerState;
use actix_web::{web, HttpRequest, HttpResponse};
use futures_util::StreamExt;
use serde_json::{json, Value};

pub(crate) async fn responses(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    read_and_dispatch(state, request, payload, "responses").await
}
pub(crate) async fn chat(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    read_and_dispatch(state, request, payload, "chat").await
}
pub(crate) async fn speech(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    read_and_dispatch(state, request, payload, "speech").await
}
pub(crate) async fn compact(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    read_and_dispatch(state, request, payload, "compact").await
}

async fn read_and_dispatch(
    state: web::Data<ServerState>,
    request: HttpRequest,
    mut payload: web::Payload,
    endpoint: &str,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    if !request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        })
    {
        return error(415, "unsupported_media_type", "此接口要求 application/json");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = payload.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => return error(400, "invalid_body", "读取请求体失败"),
        };
        if bytes.len().saturating_add(chunk.len()) > crate::MAX_REQUEST_BODY_BYTES {
            eprintln!(
                "{}",
                json!({"event":"request_body_rejected", "request_id":crate::request_identity::id(&request), "path":request.path(), "status":413, "code":"body_too_large", "received_at_least_bytes":bytes.len().saturating_add(chunk.len()), "limit_bytes":crate::MAX_REQUEST_BODY_BYTES})
            );
            return error(
                413,
                "body_too_large",
                "内联 JSON 超过 50 MiB；大附件请先上传 /v1/files，再通过 file_id 引用",
            );
        }
        bytes.extend_from_slice(&chunk);
    }
    dispatch(state, request, web::Bytes::from(bytes), endpoint).await
}

pub(crate) async fn dispatch(
    state: web::Data<ServerState>,
    request: HttpRequest,
    bytes: web::Bytes,
    endpoint: &str,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let body: Value = match serde_json::from_slice(&bytes) {
        Ok(body) => body,
        Err(_) => return error(400, "invalid_json", "请求体不是有效 JSON"),
    };
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let context = crate::request_identity::ProtocolRequest::from_http(&request, &key, &body);
    execute(state, context, body, endpoint, key).await
}
