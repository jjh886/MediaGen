use actix_web::http::header;
use actix_web::{web, HttpResponse};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use super::image::{generate_image_response, prepare_image_context};
use super::image_account::AccountQueueReporter;
use super::image_events::sse_bytes;
use super::{ImageEndpoint, OpenAiImageRequest};
use crate::AppState;

/// stream=true 时先返回 SSE 连接，再在后台排队、分配账号、执行生成。
/// started / queued / heartbeat 是本地增强事件；最终 completed 事件保持 OpenAI 图片流格式。
pub(crate) async fn start_generate_image_sse(
    state: web::Data<AppState>,
    body: OpenAiImageRequest,
    endpoint: ImageEndpoint,
) -> HttpResponse {
    let (tx, rx) = mpsc::channel::<bytes::Bytes>(32);
    let request_id = uuid::Uuid::new_v4().to_string();

    let started_event = endpoint_event(endpoint, "started");
    let _ = tx
        .send(sse_bytes(
            &started_event,
            &serde_json::json!({
                "type": started_event,
                "request_id": request_id,
                "model": body.model.clone(),
            }),
        ))
        .await;

    let heartbeat_stop = spawn_heartbeat(tx.clone());
    let state_clone = state.clone();
    let request_id_for_task = request_id.clone();
    tokio::spawn(async move {
        report_task_queue(&state_clone, &tx, &request_id_for_task, endpoint).await;

        let _queue_permit = match state_clone.image_task_queue.acquire().await {
            Ok(permit) => permit,
            Err(e) => {
                tracing::warn!(error = %e, "SSE 图片生成进入队列失败");
                send_error_and_stop(
                    &tx,
                    &heartbeat_stop,
                    &request_id_for_task,
                    "QUEUE_FULL",
                    &e.to_string(),
                )
                .await;
                return;
            }
        };

        let account_reporter = AccountQueueReporter::new(
            tx.clone(),
            request_id_for_task.clone(),
            endpoint_event(endpoint, "account_queued"),
        );
        let (ctx, account_lease) = match prepare_image_context(
            &state_clone,
            &body,
            request_id_for_task.clone(),
            endpoint,
            Some(account_reporter),
        )
        .await
        {
            Ok(v) => v,
            Err((_status, code, detail)) => {
                tracing::warn!(code = %code, detail = %detail, "SSE 准备阶段失败");
                send_error_and_stop(&tx, &heartbeat_stop, &request_id_for_task, code, &detail)
                    .await;
                return;
            }
        };

        let body = match generate_image_response(state_clone, ctx, account_lease).await {
            Ok(response) => response.to_sse_body(endpoint),
            Err((_status, code, detail)) => error_sse_body(code, &detail),
        };
        let _ = tx.send(bytes::Bytes::from(body)).await;
        heartbeat_stop.notify_one();
    });

    let stream = ReceiverStream::new(rx).map(Ok::<_, actix_web::Error>);
    HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "text/event-stream"))
        .insert_header((header::CACHE_CONTROL, "no-cache"))
        .insert_header(("Connection", "keep-alive"))
        .streaming(stream)
}

fn spawn_heartbeat(tx: mpsc::Sender<bytes::Bytes>) -> std::sync::Arc<tokio::sync::Notify> {
    let stop = std::sync::Arc::new(tokio::sync::Notify::new());
    let notify = stop.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let payload = serde_json::json!({ "type": "heartbeat" });
                    if tx.send(sse_bytes("heartbeat", &payload)).await.is_err() {
                        break;
                    }
                }
                _ = notify.notified() => break,
            }
        }
    });
    stop
}

async fn report_task_queue(
    state: &web::Data<AppState>,
    tx: &mpsc::Sender<bytes::Bytes>,
    request_id: &str,
    endpoint: ImageEndpoint,
) {
    let queued = state.image_task_queue.queued_count();
    let active = state.image_task_queue.active_count();
    let max_concurrent = state.image_task_queue.max_concurrent();
    let event = endpoint_event(endpoint, "queued");
    let _ = tx
        .send(sse_bytes(
            &event,
            &serde_json::json!({
                "type": event,
                "request_id": request_id,
                "queued": queued,
                "ahead": queued,
                "queue_position": queued + 1,
                "active": active,
                "max_concurrent": max_concurrent,
            }),
        ))
        .await;
}

async fn send_error_and_stop(
    tx: &mpsc::Sender<bytes::Bytes>,
    heartbeat_stop: &std::sync::Arc<tokio::sync::Notify>,
    request_id: &str,
    code: &str,
    detail: &str,
) {
    let payload = serde_json::json!({
        "request_id": request_id,
        "error": {
            "message": detail,
            "type": "invalid_request_error",
            "param": serde_json::Value::Null,
            "code": code,
        },
    });
    let _ = tx.send(sse_bytes("error", &payload)).await;
    heartbeat_stop.notify_one();
}

fn error_sse_body(code: &str, detail: &str) -> String {
    let payload = serde_json::json!({
        "error": {
            "message": detail,
            "type": "invalid_request_error",
            "param": serde_json::Value::Null,
            "code": code,
        },
    });
    format!("event: error\ndata: {payload}\n\n")
}

fn endpoint_event(endpoint: ImageEndpoint, suffix: &str) -> String {
    match endpoint {
        ImageEndpoint::Generation => format!("image_generation.{suffix}"),
        ImageEndpoint::Edit => format!("image_edit.{suffix}"),
    }
}
