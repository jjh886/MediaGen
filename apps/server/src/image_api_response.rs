use actix_web::HttpResponse;
#[cfg(test)]
use chrono::Utc;
#[cfg(test)]
use mediagen_core::PluginManifest;
use serde_json::{json, Value};

use crate::image_request::ImageEndpoint;
#[cfg(test)]
use crate::image_request::OpenAiImageRequest;
use crate::store::TaskRecord;

pub(crate) fn image_result_metadata(result: &Value) -> Value {
    let images = result
        .get("images")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(image_metadata).collect::<Vec<_>>())
        .unwrap_or_default();
    json!({
        "source": scalar(result.get("source")),
        "status": scalar(result.get("status")),
        "image_count": images.len(),
        "partial_image_count": result.get("partial_images").and_then(Value::as_array).map_or(0, Vec::len),
        "images": images,
    })
}

pub(crate) fn sanitize_persisted_image_tasks(tasks: &mut [TaskRecord]) -> bool {
    let mut changed = false;
    for task in tasks.iter_mut().filter(|task| task.task_kind == "image") {
        let Some(raw) = task.result_data.as_deref() else {
            continue;
        };
        let Ok(result) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        if contains_embedded_image(&result) {
            task.result_data = Some(image_result_metadata(&result).to_string());
            task.result_url = None;
            changed = true;
        }
    }
    changed
}

fn image_metadata(item: &Value) -> Value {
    json!({
        "width": scalar(item.get("width")),
        "height": scalar(item.get("height")),
        "revised_prompt": scalar(item.get("revised_prompt")),
        "usage": item.get("usage").filter(|value| value.is_object()).cloned(),
    })
}

fn scalar(value: Option<&Value>) -> Option<Value> {
    value
        .filter(|item| item.is_string() || item.is_number() || item.is_boolean())
        .cloned()
}

fn contains_embedded_image(value: &Value) -> bool {
    match value {
        Value::String(text) => text.starts_with("data:image/") || text.len() > 256 * 1024,
        Value::Array(items) => items.iter().any(contains_embedded_image),
        Value::Object(values) => values.values().any(contains_embedded_image),
        _ => false,
    }
}

#[cfg(test)]
fn new_image_task(
    task_id: &str,
    manifest: &PluginManifest,
    model_id: &str,
    body: &OpenAiImageRequest,
    account_id: &str,
) -> TaskRecord {
    TaskRecord {
        id: task_id.to_string(),
        platform: manifest.platform_id.clone(),
        task_kind: "image".to_string(),
        model_id: model_id.to_string(),
        prompt: body.prompt_text(),
        account_id: Some(account_id.to_string()),
        status: "queued".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: Utc::now().to_rfc3339(),
        completed_at: None,
    }
}

pub(crate) fn openai_error(status: u16, code: &'static str, message: &str) -> HttpResponse {
    let message = if status >= 500 {
        "上游媒体请求未成功完成，请通过请求标识检查任务"
    } else {
        message
    };
    HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status)
            .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
    )
    .json(json!({
        "error": {
            "message": message,
            "type": if status == 401 {"authentication_error"} else if status == 429 {"rate_limit_error"} else if status < 500 {"invalid_request_error"} else {"server_error"},
            "code": code,
            "param": null,
        }
    }))
}

pub(crate) fn image_completed_events(body: Value, endpoint: ImageEndpoint) -> Vec<Value> {
    let event = match endpoint {
        ImageEndpoint::Generation => "image_generation.completed",
        ImageEndpoint::Edit => "image_edit.completed",
        ImageEndpoint::Variation => "image_variation.completed",
    };
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|image| {
            let mut result = image.clone();
            result["type"] = json!(event);
            result["created_at"] = body["created"].clone();
            for key in ["background", "output_format", "quality", "size", "usage"] {
                if let Some(value) = body.get(key) {
                    result[key] = value.clone();
                }
            }
            result
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_metadata_removes_full_and_partial_base64() {
        let result = json!({
            "source": "plugin-a",
            "status": "done",
            "images": [{ "url": "data:image/png;base64,large", "width": 1024, "height": 1536 }],
            "partial_images": [{ "b64_json": "partial" }]
        });
        let metadata = image_result_metadata(&result);

        assert_eq!(metadata["image_count"], 1);
        assert_eq!(metadata["partial_image_count"], 1);
        assert_eq!(metadata["images"][0]["width"], 1024);
        assert!(!metadata.to_string().contains("base64"));
        assert!(!metadata.to_string().contains("b64_json"));
    }

    #[test]
    fn persisted_image_tasks_are_sanitized_once() {
        let mut task = new_image_task(
            "task-1",
            &test_manifest(),
            "model",
            &OpenAiImageRequest::empty(),
            "account",
        );
        task.result_data =
            Some(json!({ "images": [{ "url": "data:image/png;base64,abc" }] }).to_string());
        let mut tasks = vec![task];

        assert!(sanitize_persisted_image_tasks(&mut tasks));
        assert!(!tasks[0].result_data.as_deref().unwrap().contains("base64"));
        assert!(!sanitize_persisted_image_tasks(&mut tasks));
    }

    fn test_manifest() -> PluginManifest {
        serde_json::from_value(json!({
            "platform_id": "test",
            "display_name": "Test",
            "adapter_mode": "api"
        }))
        .unwrap()
    }
}
