use std::{collections::VecDeque, future::Future, time::Duration};

use actix_web::{web, HttpResponse};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{image_api_response::image_completed_events, image_request::ImageEndpoint};

// 将公共事件通道传到通用插件执行器，不包含平台协议或平台判断。
tokio::task_local! {
    pub(crate) static EVENTS: mpsc::Sender<Value>;
}

pub(crate) fn response(
    endpoint: ImageEndpoint,
    execution: impl Future<Output = Result<Value, (u16, &'static str, String)>> + 'static,
) -> HttpResponse {
    let (sender, receiver) = mpsc::channel(8);
    let execution = Box::pin(EVENTS.scope(sender, execution));
    let stream = futures_util::stream::unfold(
        (Some(execution), receiver, VecDeque::<Value>::new()),
        move |(mut execution, mut receiver, mut pending)| async move {
            loop {
                if let Some(event) = pending.pop_front() {
                    return Some((frame(&event), (execution, receiver, pending)));
                }
                let running = execution.as_mut()?;
                tokio::select! {
                    biased;
                    Some(event) = receiver.recv() => {
                        if let Some(event) = partial_event(event, endpoint) {
                            return Some((frame(&event), (execution, receiver, pending)));
                        }
                    }
                    result = running => {
                        execution = None;
                        match result {
                            Ok(body) => pending.extend(image_completed_events(body, endpoint)),
                            Err((status, code, _)) => pending.push_back(json!({
                                "type": "error", "code": code, "param": null,
                                "message": if status == 429 {"请求已达到限额，请稍后重试"} else {"图片生成未成功完成，请通过请求标识检查任务"}
                            })),
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {
                        return Some((Ok(web::Bytes::from_static(b": keep-alive\n\n")), (execution, receiver, pending)));
                    }
                }
            }
        },
    );
    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("cache-control", "no-cache"))
        .insert_header(("x-accel-buffering", "no"))
        .streaming(stream)
}

fn frame(event: &Value) -> Result<web::Bytes, actix_web::Error> {
    let kind = event["type"].as_str().unwrap_or("error");
    Ok(web::Bytes::from(format!(
        "event: {kind}\ndata: {event}\n\n"
    )))
}

fn partial_event(event: Value, endpoint: ImageEndpoint) -> Option<Value> {
    if event["type"] != "image.partial" || !event["b64_json"].is_string() {
        return None;
    }
    let kind = match endpoint {
        ImageEndpoint::Generation => "image_generation.partial_image",
        ImageEndpoint::Edit => "image_edit.partial_image",
        ImageEndpoint::Variation => return None,
    };
    let mut result = event;
    result["type"] = json!(kind);
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[actix_web::test]
    async fn forwards_partial_before_completion_and_uses_image_schema() {
        let response = response(ImageEndpoint::Edit, async {
            EVENTS.with(|sender| sender.try_send(json!({"type":"image.partial", "b64_json":"preview", "partial_image_index":0})).unwrap());
            tokio::task::yield_now().await;
            Ok(json!({"created":123,"data":[{"b64_json":"final"}],"usage":{"total_tokens":42}}))
        });
        let bytes = actix_web::body::to_bytes(response.into_body())
            .await
            .unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(
            text.find("image_edit.partial_image").unwrap()
                < text.find("image_edit.completed").unwrap()
        );
        assert!(!text.contains("[DONE]"));
        assert!(text.contains("\"total_tokens\":42"));
        assert!(!text.contains("\"data\":["));
    }

    #[test]
    fn ignores_non_image_plugin_events() {
        assert!(partial_event(
            json!({"type":"response.completed"}),
            ImageEndpoint::Generation
        )
        .is_none());
    }
}
