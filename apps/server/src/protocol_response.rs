use super::{error, public_failure};
use actix_web::HttpResponse;
use base64::prelude::{Engine as _, BASE64_STANDARD};
use serde_json::Value;
use tokio::sync::mpsc;

pub(crate) struct AbortJob {
    pub(super) handle: tokio::task::JoinHandle<Result<Value, String>>,
    pub(super) detach: bool,
}
impl Drop for AbortJob {
    fn drop(&mut self) {
        if !self.detach {
            self.handle.abort();
        }
    }
}

pub(super) fn invalid_upstream_response(value: &Value, is_audio: bool, streaming: bool) -> bool {
    if is_audio {
        value["audio_base64"]
            .as_str()
            .is_none_or(|audio| BASE64_STANDARD.decode(audio).is_err())
    } else {
        value["response"]["id"].as_str().is_none()
            || !value["response"]["output"].is_array()
            || (streaming && value.get("terminal_event").is_none())
    }
}

pub(super) async fn respond(
    mut job: AbortJob,
    receiver: mpsc::Receiver<Value>,
    streaming: bool,
    endpoint: &str,
    model_id: String,
    include_usage: bool,
    request_id: String,
) -> HttpResponse {
    if streaming {
        return crate::protocol_stream::response(
            receiver,
            job,
            endpoint,
            model_id,
            include_usage,
            request_id,
        )
        .await;
    }
    drop(receiver);
    match (&mut job.handle).await {
        Ok(Ok(result)) if endpoint == "speech" => {
            let audio = result["audio_base64"]
                .as_str()
                .and_then(|value| BASE64_STANDARD.decode(value).ok());
            match audio.filter(|value| !value.is_empty()) {
                Some(audio) => HttpResponse::Ok()
                    .insert_header(("x-request-id", request_id))
                    .content_type(
                        result["mime_type"]
                            .as_str()
                            .unwrap_or("application/octet-stream"),
                    )
                    .body(audio),
                None => error(502, "invalid_audio", "上游没有返回有效音频"),
            }
        }
        Ok(Ok(result)) => {
            let response = if endpoint == "chat" {
                crate::protocol_request::chat_response(&result["response"], &model_id)
            } else {
                result["response"].clone()
            };
            HttpResponse::Ok()
                .insert_header(("x-request-id", request_id))
                .json(response)
        }
        Ok(Err(message)) => {
            let (status, code, public) = public_failure(&message);
            error(status, code, &public)
        }
        Err(_) => error(500, "execution_failed", "请求执行中断"),
    }
}
