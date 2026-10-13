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
    let mut task = new_video_task(
        &task_id,
        context.manifest,
        &context.model.model_id,
        &body,
        &context.account.id,
        &owner_hash,
    );
    let mut metadata: Value =
        serde_json::from_str(task.result_data.as_deref().unwrap_or("{}")).unwrap_or_default();
    metadata["_recovery_ciphertext"] = serde_json::json!(state
        .credential_cipher
        .encrypt(&serde_json::json!({"request":body,"caller_key":caller_key}).to_string())
        .map_err(|_| openai_error(500, "storage_error", "保存视频恢复请求失败"))?);
    task.result_data = Some(metadata.to_string());
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
        run_recoverable_video(&state, body, caller_key, task_id, first_account_id).await;
    });
    response
}

fn api_error_response((status, code, message): ApiError) -> HttpResponse {
    openai_error(status, code, &message)
}

async fn run_recoverable_video(
    state: &web::Data<ServerState>,
    body: OpenAiVideoRequest,
    key: String,
    id: String,
    account: String,
) {
    loop {
        match run_video_task(
            state,
            body.clone(),
            key.clone(),
            id.clone(),
            account.clone(),
        )
        .await
        {
            Ok(_) => break,
            Err((_, "task_cancelled", _)) => break,
            Err((_, "temporary_unavailable", _)) => {
                state
                    .store
                    .add_task_event(&id, "info", "等待恢复原视频任务".into());
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
            Err((_, _, message)) => {
                state.store.fail_task(&id, message);
                break;
            }
        }
    }
}

fn saved_video(
    state: &ServerState,
    task: &crate::store::TaskRecord,
) -> Result<(OpenAiVideoRequest, String, String), String> {
    let metadata: Value = serde_json::from_str(task.result_data.as_deref().unwrap_or("{}"))
        .map_err(|_| "视频恢复记录无效")?;
    let ciphertext = metadata["_recovery_ciphertext"]
        .as_str()
        .ok_or("视频未保存恢复请求")?;
    let value: Value = serde_json::from_str(&state.credential_cipher.decrypt(ciphertext)?)
        .map_err(|_| "视频恢复请求无效")?;
    let key = value["caller_key"]
        .as_str()
        .ok_or("视频恢复缺少调用方")?
        .to_owned();
    if !crate::video_api_response::task_owned_by(task, &hash_secret(&key)) {
        return Err("视频恢复记录归属不符".into());
    }
    let account = state
        .store
        .account_by_id(task.account_id.as_deref().ok_or("视频原账号缺失")?)
        .ok_or("视频原账号不存在")?;
    if !state.store.global_api_key_allowed(&key)
        && !crate::access_control::account_key_allowed(&account.api_key, &key)
    {
        return Err("视频调用方授权已失效".into());
    }
    let body: OpenAiVideoRequest =
        serde_json::from_value(value["request"].clone()).map_err(|_| "视频恢复参数无效")?;
    body.validate().map_err(|(_, _, message)| message)?;
    Ok((body, key, account.id))
}

pub(crate) fn recover_pending(state: web::Data<ServerState>) {
    for task in state.store.list_tasks(None).into_iter().filter(|task| {
        task.task_kind == "video" && matches!(task.status.as_str(), "queued" | "processing")
    }) {
        let (body, key, account) = match saved_video(&state, &task) {
            Ok(value) => value,
            Err(message) => {
                state.store.fail_task(&task.id, message);
                continue;
            }
        };
        let state = state.clone();
        actix_web::rt::spawn(async move {
            loop {
                if crate::media_execution::ensure_task_not_cancelled(&state, &task.id).is_err() {
                    return;
                }
                let settings = state.store.settings();
                if let Ok(reservation) = state
                    .video_task_queue
                    .reserve_with_limit(settings.video_task_queue_limit, "视频")
                {
                    let _permit = reservation
                        .activate_with_concurrency(settings.video_task_concurrency)
                        .await;
                    run_recoverable_video(&state, body, key, task.id, account).await;
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[actix_web::test]
    async fn video_request_and_upstream_checkpoint_survive_restart_but_cancelled_tasks_stay_cancelled(
    ) {
        for cancelled in [false, true] {
            let mut state = crate::video_api_tests::video_state(
                "restart",
                "https://example.test/original.mp4",
                0,
            );
            let root = state.store.path.parent().unwrap().to_owned();
            state.credential_cipher =
                Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
            let mut manifest = state
                .plugin_loader
                .get_plugin("fake-video")
                .unwrap()
                .clone();
            manifest.flows.get_mut("generate_video").unwrap().steps[0]["script"] = serde_json::json!(
                "function main(){const saved=host.checkpoint();if(saved.conversation_id!=='original')throw new Error('must not submit');return {videos:[{video_url:'https://example.test/original.mp4'}]}}"
            );
            let directory = state.plugin_loader.get_plugin_dir("fake-video").unwrap();
            std::fs::write(
                directory.join("manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            state.plugin_loader = Arc::new(
                mediagen_plugin_runtime::PluginLoader::load_from(directory.parent().unwrap())
                    .unwrap(),
            );
            let mut state = web::Data::new(state);
            let body: OpenAiVideoRequest =
                serde_json::from_value(serde_json::json!({"model":"fake-video","prompt":"原请求"}))
                    .unwrap();
            let submission = prepare_video_submission(
                &state,
                AuthorizedVideoRequest {
                    body,
                    caller_key: "video-key".into(),
                    owner_hash: hash_secret("video-key"),
                },
            )
            .unwrap();
            let id = submission.task_id.clone();
            let account = state
                .store
                .account_by_id(&submission.first_account_id)
                .unwrap();
            let host = crate::server_plugin_host::ServerPluginHost::new_for_account(&account)
                .with_storage(
                    state.credential_cipher.clone(),
                    &account.profile_dir,
                    &manifest,
                )
                .with_events(None, id.clone());
            host.checkpoint(
                &serde_json::json!({"value":{"phase":"accepted","conversation_id":"original"}}),
            )
            .await
            .unwrap();
            state.store.start_task(&id);
            if cancelled {
                state.store.cancel_task(&id);
            }
            drop(submission);
            let mut restarted = state.get_ref().clone();
            restarted.store =
                Arc::new(crate::store::AppStore::load(root.join("server-state.json")));
            restarted.credential_cipher =
                Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
            state = web::Data::new(restarted);
            recover_pending(state.clone());
            if cancelled {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                assert_eq!(state.store.task_by_id(&id).unwrap().status, "cancelled");
                assert_eq!(
                    state.store.account_by_id(&account.id).unwrap().used_total,
                    0
                );
            } else {
                tokio::time::timeout(std::time::Duration::from_secs(3), async {
                    while state.store.task_by_id(&id).unwrap().status != "completed" {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                let task = state.store.task_by_id(&id).unwrap();
                assert!(task.result_data.unwrap().contains("original.mp4"));
                assert_eq!(
                    state.store.account_by_id(&account.id).unwrap().used_total,
                    1
                );
            }
            std::fs::remove_dir_all(root).unwrap();
            let _ = std::fs::remove_dir_all(account.profile_dir);
        }
    }
}
