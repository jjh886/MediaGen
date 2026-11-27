use super::{error, execute};
use crate::ServerState;
use actix_web::{web, HttpRequest, HttpResponse};
use serde_json::Value;

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
    payload: web::Payload,
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
    let body = match crate::media_multipart::read_json(payload).await {
        Ok(body) => body,
        Err((status, code, message)) => return error(status, code, &message),
    };
    dispatch(state, request, body, endpoint).await
}

pub(crate) async fn dispatch(
    state: web::Data<ServerState>,
    request: HttpRequest,
    body: Value,
    endpoint: &str,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let mut context = crate::request_identity::ProtocolRequest::from_http(&request, &key, &body);
    if endpoint == "responses"
        && request
            .headers()
            .get("x-mediagen-receipt")
            .is_some_and(|v| v == "1")
    {
        if body["async"] == true {
            return error(400, "invalid_request", "异步请求使用生成任务回执");
        }
        let Some(id) = request
            .headers()
            .get("x-client-request-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| uuid::Uuid::parse_str(v).ok())
        else {
            return error(400, "invalid_request_id", "恢复请求需要 UUID");
        };
        context.task_id = format!("receipt_{}", context.task_id);
        // 连接可在排队或返回响应头之前断开，已受理的调用仍应运行到终态。
        return actix_web::rt::spawn(async move {
            execute(state, context, body, "responses", key, Some(id)).await
        })
        .await
        .unwrap_or_else(|_| error(500, "execution_failed", "请求执行中断"));
    }
    execute(state, context, body, endpoint, key, None).await
}
