use actix_web::{web, HttpRequest, HttpResponse};

use super::image::{generate_image_json, prepare_image_context};
use super::image_multipart::parse_edit_request;
use super::image_sse::start_generate_image_sse;
use super::{error_response, ImageEndpoint, OpenAiImageRequest};
use crate::AppState;

pub(crate) async fn generate_image(
    state: web::Data<AppState>,
    body: web::Json<OpenAiImageRequest>,
) -> HttpResponse {
    handle_image_request(state, body.into_inner(), ImageEndpoint::Generation).await
}

pub(crate) async fn edit_image(
    state: web::Data<AppState>,
    req: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    let body = match parse_edit_request(req, payload).await {
        Ok(body) => body,
        Err((status, code, detail)) => return error_response(status, code, &detail),
    };
    handle_image_request(state, body, ImageEndpoint::Edit).await
}

pub(crate) async fn image_variation() -> HttpResponse {
    error_response(
        501,
        "unsupported_endpoint",
        "MediaGen 当前插件流程暂不支持 /v1/images/variations",
    )
}

async fn handle_image_request(
    state: web::Data<AppState>,
    mut body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
) -> HttpResponse {
    body.apply_endpoint_defaults(endpoint);
    if let Err((status, code, detail)) = body.validate(endpoint) {
        return error_response(status, code, &detail);
    }

    if body.stream == Some(true) {
        return start_generate_image_sse(state, body, endpoint).await;
    }

    let queue_permit = match state.image_task_queue.acquire().await {
        Ok(permit) => permit,
        Err(e) => {
            tracing::warn!(error = %e, "图片生成同步请求进入队列失败");
            return error_response(429, "QUEUE_FULL", &e.to_string());
        }
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    match prepare_image_context(&state, &body, request_id, None).await {
        Ok((ctx, account_lease)) => {
            let _queue_permit = queue_permit;
            generate_image_json(state, ctx, account_lease).await
        }
        Err((status, code, detail)) => error_response(status, code, &detail),
    }
}
