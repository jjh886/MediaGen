use actix_web::{web, HttpResponse};

use super::image::{generate_image_json, prepare_image_context};
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
    body: web::Json<OpenAiImageRequest>,
) -> HttpResponse {
    handle_image_request(state, body.into_inner(), ImageEndpoint::Edit).await
}

pub(crate) async fn image_variation() -> HttpResponse {
    error_response(
        501,
        "unsupported_endpoint",
        "Frame64 当前插件流程暂不支持 /v1/images/variations",
    )
}

async fn handle_image_request(
    state: web::Data<AppState>,
    body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
) -> HttpResponse {
    if let Err((status, code, detail)) = body.validate(endpoint) {
        return error_response(status, code, &detail);
    }

    let _queue_permit = match state.image_task_queue.acquire().await {
        Ok(permit) => permit,
        Err(e) => {
            tracing::warn!(error = %e, "图片生成同步请求进入队列失败");
            return error_response(429, "QUEUE_FULL", &e.to_string());
        }
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    match prepare_image_context(&state, &body, request_id).await {
        Ok(ctx) => generate_image_json(state, ctx).await,
        Err((status, code, detail)) => error_response(status, code, &detail),
    }
}
