use actix_web::{web, HttpRequest, HttpResponse, Responder};
use mediagen_core::TaskKind;
use serde_json::Value;
use uuid::Uuid;

use crate::access_control::hash_secret;
use crate::image_api_response::openai_error;
use crate::image_task_queue::ImageTaskReservation;
use crate::media_execution::{resolve_media_flow_context, ApiError};
use crate::routes::{api_key_allowed, api_key_from_request};
use crate::video_api::run_video_task;
use crate::video_api_response::{new_video_task, openai_video};
use crate::video_multipart::parse_video_request;
use crate::video_request::{validate_video_capabilities, OpenAiVideoRequest};
use crate::ServerState;

struct AuthorizedVideoRequest {
    body: OpenAiVideoRequest,
    caller_key: String,
    owner_hash: String,
}

struct VideoSubmission {
    body: OpenAiVideoRequest,
    caller_key: String,
    task_id: String,
    first_account_id: String,
    reservation: ImageTaskReservation,
    concurrency: u32,
    response: Value,
}

pub async fn create_video(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> impl Responder {
    let authorized = match authorize_video_request(&state, request, payload).await {
        Ok(authorized) => authorized,
        Err(response) => return response,
    };
    let submission = match prepare_video_submission(&state, authorized) {
        Ok(submission) => submission,
        Err(response) => return response,
    };
    HttpResponse::Ok().json(spawn_video_submission(state, submission))
}

async fn authorize_video_request(
    state: &web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> Result<AuthorizedVideoRequest, HttpResponse> {
    if !api_key_allowed(state, &request) {
        return Err(openai_error(401, "authentication_error", "invalid api key"));
    }
    let caller_key = api_key_from_request(&request)
        .ok_or_else(|| openai_error(401, "authentication_error", "invalid api key"))?;
    let mut body = parse_video_request(request, payload)
        .await
        .map_err(api_error_response)?;
    let owner_hash = hash_secret(&caller_key);
    crate::video_file_refs::resolve_openai_video_file_refs(
        &state.file_root,
        &mut body,
        &owner_hash,
    )
    .await
    .map_err(api_error_response)?;
    body.validate().map_err(api_error_response)?;
    Ok(AuthorizedVideoRequest {
        body,
        caller_key,
        owner_hash,
    })
}

fn prepare_video_submission(
    state: &web::Data<ServerState>,
    authorized: AuthorizedVideoRequest,
) -> Result<VideoSubmission, HttpResponse> {
    let AuthorizedVideoRequest {
        mut body,
        caller_key,
        owner_hash,
    } = authorized;
    let settings = state.store.settings();
    let reservation = state
        .video_task_queue
        .reserve_with_limit(settings.video_task_queue_limit, "视频")
        .map_err(|message| openai_error(429, "queue_full", &message))?;
    let task_id = format!("video_{}", Uuid::new_v4().simple());
    let context = state
        .store
        .with_account_dispatch(|| {
            resolve_media_flow_context(
                state,
                &body.model,
                TaskKind::Video,
                Some(&caller_key),
                task_id.clone(),
            )
        })
        .map_err(api_error_response)?;
    body.apply_manifest_defaults(context.manifest);
    validate_video_capabilities(context.manifest, &body).map_err(api_error_response)?;
    let task = new_video_task(
        &task_id,
        context.manifest,
        &context.model.model_id,
        &body,
        &context.account.id,
        &owner_hash,
    );
    let first_account_id = context.account.id.clone();
    let response = openai_video(&task);
    state.store.insert_task(task);
    state
        .store
        .add_task_event(&task_id, "info", "视频任务进入队列".to_string());
    Ok(VideoSubmission {
        body,
        caller_key,
        task_id,
        first_account_id,
        reservation,
        concurrency: settings.video_task_concurrency,
        response,
    })
}

fn spawn_video_submission(state: web::Data<ServerState>, submission: VideoSubmission) -> Value {
    let VideoSubmission {
        body,
        caller_key,
        task_id,
        first_account_id,
        reservation,
        concurrency,
        response,
    } = submission;
    actix_web::rt::spawn(async move {
        let _permit = reservation.activate_with_concurrency(concurrency).await;
        if let Err((_status, _code, message)) =
            run_video_task(&state, body, caller_key, task_id.clone(), first_account_id).await
        {
            eprintln!("[mediagen-server] 视频任务 {task_id} 结束: {message}");
        }
    });
    response
}

fn api_error_response((status, code, message): ApiError) -> HttpResponse {
    openai_error(status, code, &message)
}
