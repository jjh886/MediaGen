use std::time::Duration;

use chrono::Utc;
use reqwest::header::USER_AGENT;
use serde_json::{json, Value};

use crate::image_request::OpenAiImageRequest;
use crate::store::GenerationJobState;
use crate::ServerState;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(15);
const CALLBACK_ATTEMPTS: usize = 3;
const CALLBACK_USER_AGENT: &str = "MediaGen-Generation-Callback/1.0";

pub(crate) struct CallbackInput {
    pub(crate) task_id: String,
    pub(crate) body: CallbackTarget,
}

#[derive(Clone)]
pub(crate) struct CallbackTarget {
    model: String,
    url: Option<String>,
    kind: &'static str,
}
impl From<OpenAiImageRequest> for CallbackTarget {
    fn from(body: OpenAiImageRequest) -> Self {
        Self {
            model: body.model.clone(),
            url: body.callback_url(),
            kind: "image_generation",
        }
    }
}

pub(crate) fn validate_url(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 2048 {
        return Err("callback_url 长度必须在 1 到 2048 之间".into());
    }
    let url = reqwest::Url::parse(value).map_err(|_| "callback_url 格式无效")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("callback_url 必须是 http/https 地址，且不能包含用户名或密码".into());
    }
    Ok(())
}

fn from_job(job: &crate::store::GenerationJobRecord) -> Option<CallbackTarget> {
    if job.task_kind == "image" {
        return serde_json::from_str::<OpenAiImageRequest>(&job.request_json)
            .ok()
            .map(Into::into);
    }
    if job.task_kind != "text" {
        return None;
    }
    let value: Value = serde_json::from_str(&job.request_json).ok()?;
    Some(CallbackTarget {
        model: value["body"]["model"].as_str()?.into(),
        url: value["body"]["callback_url"]
            .as_str()
            .map(|url| url.trim().to_owned()),
        kind: "text_generation",
    })
}

pub(super) fn target(
    state: &ServerState,
    task_id: &str,
    image: Option<&OpenAiImageRequest>,
) -> Option<CallbackTarget> {
    image
        .cloned()
        .map(Into::into)
        .or_else(|| from_job(&state.store.generation_job(task_id, None)?))
}

pub(crate) fn pending_cancel_callbacks(
    state: &ServerState,
    owner: Option<&str>,
    task_ids: &[String],
) -> Vec<CallbackInput> {
    task_ids
        .iter()
        .filter_map(|task_id| callback_input_for_pending_job(state, owner, task_id))
        .collect()
}

pub(crate) fn spawn_completed(
    state: ServerState,
    task_id: String,
    body: impl Into<CallbackTarget>,
    data: Value,
) {
    let body = body.into();
    let payload = json!({
        "object": "generation.task.callback",
        "event": format!("{}.completed", body.kind),
        "task_id": task_id,
        "status": "completed",
        "created": Utc::now().timestamp(),
        "model": body.model.clone(),
        "data": data,
    });
    spawn_delivery(state, task_id, body, payload);
}

pub(crate) fn spawn_failed(
    state: ServerState,
    task_id: String,
    body: impl Into<CallbackTarget>,
    code: &str,
    message: &str,
) {
    let body = body.into();
    let payload = json!({
        "object": "generation.task.callback",
        "event": format!("{}.failed", body.kind),
        "task_id": task_id,
        "status": "failed",
        "created": Utc::now().timestamp(),
        "model": body.model.clone(),
        "error": { "code": code, "message": message },
    });
    spawn_delivery(state, task_id, body, payload);
}

pub(crate) fn spawn_cancelled(
    state: ServerState,
    task_id: String,
    body: impl Into<CallbackTarget>,
) {
    let body = body.into();
    let payload = json!({
        "object": "generation.task.callback",
        "event": format!("{}.cancelled", body.kind),
        "task_id": task_id,
        "status": "cancelled",
        "created": Utc::now().timestamp(),
        "model": body.model.clone(),
        "error": { "code": "task_cancelled", "message": "用户取消" },
    });
    spawn_delivery(state, task_id, body, payload);
}

pub(crate) fn spawn_cancelled_batch(state: ServerState, items: Vec<CallbackInput>) {
    for item in items {
        spawn_cancelled(state.clone(), item.task_id, item.body);
    }
}

fn callback_input_for_pending_job(
    state: &ServerState,
    owner: Option<&str>,
    task_id: &str,
) -> Option<CallbackInput> {
    let job = state.store.generation_job(task_id, owner)?;
    if !matches!(
        job.state,
        GenerationJobState::Queued | GenerationJobState::RetryWait
    ) {
        return None;
    }
    let body = from_job(&job)?;
    body.url.as_ref()?;
    Some(CallbackInput {
        task_id: task_id.to_string(),
        body,
    })
}

fn spawn_delivery(state: ServerState, task_id: String, body: CallbackTarget, payload: Value) {
    let Some(callback_url) = body.url else {
        return;
    };
    actix_web::rt::spawn(async move {
        let result = post_callback_with_retries(&callback_url, &payload).await;
        record_delivery_result(&state, &task_id, &callback_url, result);
    });
}

async fn post_callback_with_retries(callback_url: &str, payload: &Value) -> Result<(), String> {
    validate_url(callback_url)?;
    let mut last_error = String::new();
    for attempt in 1..=CALLBACK_ATTEMPTS {
        match post_callback_once(callback_url, payload).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        if attempt < CALLBACK_ATTEMPTS {
            tokio::time::sleep(Duration::from_secs(attempt as u64)).await;
        }
    }
    Err(last_error)
}

async fn post_callback_once(callback_url: &str, payload: &Value) -> Result<(), String> {
    // 回调失败只记录事件，不反向改变生成任务终态，避免结果已生成却被误判为失败。
    let client = reqwest::Client::builder()
        .timeout(CALLBACK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.without_url().to_string())?;
    let response = client
        .post(callback_url)
        .header(USER_AGENT, CALLBACK_USER_AGENT)
        .json(payload)
        .send()
        .await
        .map_err(|error| error.without_url().to_string())?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {status}"))
    }
}

fn record_delivery_result(
    state: &ServerState,
    task_id: &str,
    callback_url: &str,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => state
            .store
            .add_task_event(task_id, "info", "异步回调通知成功".to_string()),
        Err(error) => {
            let summary = format!("异步回调通知失败: {error}");
            state.store.add_task_event(task_id, "warn", summary.clone());
            state.store.add_audit_log(
                "engine",
                "warn",
                "异步回调通知失败".to_string(),
                format!(
                    "task={task_id}, url={}, error={error}",
                    safe_url(callback_url)
                ),
                Some(task_id.to_string()),
            );
        }
    }
}

fn safe_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return "<invalid callback_url>".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_url_removes_sensitive_parts() {
        assert_eq!(
            safe_url("https://example.test/cb?token=secret#frag"),
            "https://example.test/cb"
        );
    }

    #[actix_web::test]
    async fn failed_text_posts_terminal_callback_without_success_data() {
        use actix_web::{test, App};
        let (url, received) = crate::test_fixtures::callback_receiver().await;
        let state = crate::protocol_api_tests::state();
        let directory = state.plugin_loader.get_plugin_dir("test").unwrap();
        std::fs::write(
            directory.join("scripts/text.js"),
            "function main() { throw new Error('content_policy_refusal: upstream rejected'); }",
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let accepted: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .set_json(
                    json!({"model":"text-model","input":"test","async":true,"callback_url":url}),
                )
                .to_request(),
        )
        .await;
        let id = accepted["data"]["taskId"].as_str().unwrap();
        super::super::worker::dispatch_available(&state).await;
        let callback = crate::test_fixtures::read_callback_json(received).await;
        assert_eq!(callback["event"], "text_generation.failed");
        assert_eq!(callback["task_id"], id);
        assert_eq!(callback["status"], "failed");
        assert!(callback.get("data").is_none());
        assert!(callback["error"]["message"].as_str().is_some());
        assert_eq!(
            state.store.generation_job(id, None).unwrap().state,
            GenerationJobState::Failed
        );
    }

    #[actix_web::test]
    async fn callback_retries_same_payload_and_failure_preserves_completed_result() {
        use tokio::io::AsyncWriteExt;
        for succeeds in [true, false] {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .unwrap();
            let url = format!(
                "http://{}/callback?token=secret",
                listener.local_addr().unwrap()
            );
            let receiver = tokio::spawn(async move {
                let mut bodies = Vec::new();
                for attempt in 0..3 {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    bodies.push(crate::test_fixtures::read_callback_body(&mut stream).await);
                    let response = if succeeds && attempt == 2 {
                        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".as_slice()
                    } else {
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n".as_slice()
                    };
                    stream.write_all(response).await.unwrap();
                }
                bodies
            });
            let state = crate::protocol_api_tests::state();
            let body = json!({"model":"text-model","input":"test","async":true});
            let request = actix_web::test::TestRequest::post()
                .uri("/v1/responses")
                .to_http_request();
            let context =
                crate::request_identity::ProtocolRequest::from_http(&request, "key-a", &body);
            let id = context.task_id.clone();
            super::super::text::enqueue_text(
                state.clone(),
                "key-a".into(),
                body,
                "responses",
                context,
            )
            .await;
            let data = json!({"object":"response","output":[]});
            super::super::result::save(&state, &id, &data)
                .await
                .unwrap();
            state.store.complete_task(&id, data.clone());
            state
                .store
                .finish_generation_job(&id, GenerationJobState::Completed, None);
            let payload = json!({"task_id":id,"event":"text_generation.completed","data":data});
            let result = tokio::time::timeout(
                Duration::from_secs(6),
                post_callback_with_retries(&url, &payload),
            )
            .await
            .unwrap();
            assert_eq!(result.is_ok(), succeeds);
            record_delivery_result(&state, &id, &url, result);
            let bodies = receiver.await.unwrap();
            assert_eq!(bodies.len(), 3);
            for body in bodies {
                assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), payload);
            }
            assert_eq!(
                state.store.generation_job(&id, None).unwrap().state,
                GenerationJobState::Completed
            );
            assert_eq!(
                super::super::result::read(&state, &id).await.unwrap(),
                Some(data)
            );
        }
    }
}
