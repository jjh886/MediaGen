use chrono::{DateTime, Utc};
use mediagen_core::PluginManifest;
use serde_json::{json, Value};

use crate::store::TaskRecord;
use crate::video_request::OpenAiVideoRequest;

pub(crate) fn new_video_task(
    task_id: &str,
    manifest: &PluginManifest,
    model_id: &str,
    body: &OpenAiVideoRequest,
    account_id: &str,
    owner_hash: &str,
) -> TaskRecord {
    TaskRecord {
        id: task_id.to_string(),
        platform: manifest.platform_id.clone(),
        task_kind: "video".to_string(),
        model_id: model_id.to_string(),
        prompt: body.prompt.clone(),
        account_id: Some(account_id.to_string()),
        status: "queued".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: Some(
            json!({
                "_owner_hash": owner_hash,
                "seconds": body.seconds_text().unwrap_or_else(|_| "4".to_string()),
                "size": body.size_text(),
                "quality": body.quality_text(),
            })
            .to_string(),
        ),
        created_at: Utc::now().to_rfc3339(),
        completed_at: None,
    }
}

pub(crate) fn completed_video_metadata(
    task: &TaskRecord,
    body: &OpenAiVideoRequest,
    result: &Value,
) -> Value {
    let video = first_video(result).unwrap_or(Value::Null);
    json!({
        "_owner_hash": owner_hash(task),
        "seconds": body.seconds_text().unwrap_or_else(|_| "4".to_string()),
        "size": body.size_text(),
        "quality": body.quality_text(),
        "video": video,
        "source": result.get("source").cloned(),
    })
}

pub(crate) fn result_has_video(result: &Value) -> bool {
    first_video(result).is_some_and(|video| {
        ["url", "video_url", "download_url"]
            .iter()
            .any(|field| video.get(*field).and_then(Value::as_str).is_some())
    })
}

pub(crate) fn openai_video(task: &TaskRecord) -> Value {
    let metadata = task_metadata(task);
    let status = openai_status(&task.status);
    let error = (status == "failed").then(|| {
        if let Some(message) = task.error_message.as_deref() {
            if message.contains("account_permission_denied")
                || message.contains("insufficient_quota")
            {
                let (_, code, public) = crate::protocol_api::public_failure(message);
                return json!({"code":code,"message":public});
            }
        }
        json!({
            "code": error_code(task),
            "message": task.error_message.clone().unwrap_or_else(|| "视频生成失败".to_string()),
        })
    });
    json!({
        "id": task.id,
        "object": "video",
        "model": task.model_id,
        "status": status,
        "progress": progress(&task.status),
        "created_at": unix_time(&task.created_at),
        "completed_at": task.completed_at.as_deref().map(unix_time),
        "expires_at": Value::Null,
        "error": error,
        "prompt": task.prompt,
        "seconds": metadata.get("seconds").and_then(Value::as_str).unwrap_or("4"),
        "size": metadata.get("size").and_then(Value::as_str).unwrap_or("720x1280"),
        "quality": metadata.get("quality").and_then(Value::as_str).unwrap_or("standard"),
        "remixed_from_video_id": Value::Null,
    })
}

pub(crate) fn task_owned_by(task: &TaskRecord, owner_hash_value: &str) -> bool {
    !owner_hash_value.is_empty() && owner_hash(task) == owner_hash_value
}

pub(crate) fn video_asset_url(task: &TaskRecord, variant: &str) -> Option<String> {
    let metadata = task_metadata(task);
    let video = metadata.get("video")?;
    let field_names: &[&str] = match variant {
        "thumbnail" => &["cover_url", "thumbnail_url"],
        "video" | "" => &["url", "video_url", "download_url"],
        _ => return None,
    };
    field_names.iter().find_map(|field| {
        video
            .get(*field)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

fn task_metadata(task: &TaskRecord) -> Value {
    task.result_data
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or(Value::Null)
}

fn owner_hash(task: &TaskRecord) -> String {
    task_metadata(task)
        .get("_owner_hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn first_video(result: &Value) -> Option<Value> {
    if let Some(video) = result
        .get("videos")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    {
        return Some(video.clone());
    }
    let url = result
        .get("video_url")
        .or_else(|| result.get("download_url"))
        .or_else(|| result.get("url"))
        .and_then(Value::as_str)?;
    Some(json!({
        "url": url,
        "cover_url": result.get("cover_url").cloned(),
        "duration": result.get("duration").cloned(),
        "width": result.get("width").cloned(),
        "height": result.get("height").cloned(),
    }))
}

fn openai_status(status: &str) -> &'static str {
    match status {
        "queued" => "queued",
        "processing" => "in_progress",
        "completed" => "completed",
        _ => "failed",
    }
}

fn progress(status: &str) -> u32 {
    match status {
        "queued" => 0,
        "processing" => 50,
        "completed" => 100,
        _ => 0,
    }
}

fn error_code(task: &TaskRecord) -> &'static str {
    if task.status == "cancelled" {
        "video_cancelled"
    } else if task.status == "verification_required" {
        "verification_required"
    } else {
        "video_generation_failed"
    }
}

fn unix_time(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_task_maps_to_openai_video_and_content_url() {
        let mut request = OpenAiVideoRequest::empty();
        request.model = "video-model".to_string();
        request.prompt = "move".to_string();
        let manifest: PluginManifest = serde_json::from_value(json!({
            "platform_id": "test",
            "display_name": "Test"
        }))
        .unwrap();
        let mut task = new_video_task("video-1", &manifest, "video-model", &request, "a", "owner");
        task.status = "completed".to_string();
        task.result_data = Some(
            completed_video_metadata(
                &task,
                &request,
                &json!({ "videos": [{ "video_url": "https://example.test/video.mp4" }] }),
            )
            .to_string(),
        );

        assert_eq!(openai_video(&task)["status"], "completed");
        assert_eq!(
            video_asset_url(&task, "video").as_deref(),
            Some("https://example.test/video.mp4")
        );
        assert!(task_owned_by(&task, "owner"));
    }

    #[test]
    fn verification_task_uses_stable_openai_error_code() {
        let mut request = OpenAiVideoRequest::empty();
        request.model = "video-model".to_string();
        request.prompt = "move".to_string();
        let manifest: PluginManifest = serde_json::from_value(json!({
            "platform_id": "test",
            "display_name": "Test"
        }))
        .unwrap();
        let mut task = new_video_task(
            "video-verify",
            &manifest,
            "video-model",
            &request,
            "a",
            "owner",
        );
        task.status = "verification_required".to_string();
        task.error_message = Some("平台要求人工验证（710022004）".to_string());

        let response = openai_video(&task);

        assert_eq!(response["status"], "failed");
        assert_eq!(response["error"]["code"], "verification_required");
    }
}
