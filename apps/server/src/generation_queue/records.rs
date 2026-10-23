use crate::image_request::OpenAiImageRequest;
use crate::store::{GenerationJobRecord, GenerationJobState, TaskRecord};

use super::owner_key;

/// 生产入队与测试共用同一构造器，避免任务记录和调度记录的字段规则漂移。
pub(crate) fn queued_image_records(
    task_id: String,
    platform: String,
    body: &OpenAiImageRequest,
    caller_key: &str,
    credential_ciphertext: String,
    now: String,
) -> Result<(TaskRecord, GenerationJobRecord), serde_json::Error> {
    let task = TaskRecord {
        id: task_id.clone(),
        platform,
        task_kind: "image".to_string(),
        model_id: body.model.clone(),
        prompt: body.prompt_text(),
        account_id: None,
        status: "pending".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: now.clone(),
        completed_at: None,
    };
    Ok(queued_records(
        task,
        serde_json::to_string(body)?,
        caller_key,
        credential_ciphertext,
    ))
}

pub(super) fn queued_records(
    task: TaskRecord,
    request_json: String,
    caller_key: &str,
    credential_ciphertext: String,
) -> (TaskRecord, GenerationJobRecord) {
    let now = task.created_at.clone();
    let job = GenerationJobRecord {
        task_id: task.id.clone(),
        owner_key: owner_key(caller_key),
        task_kind: task.task_kind.clone(),
        request_json,
        credential_ciphertext,
        state: GenerationJobState::Queued,
        priority: 100,
        queue_seq: 0,
        next_attempt_at: now.clone(),
        retry_count: 0,
        last_failure_category: None,
        last_error_summary: None,
        lease_until: None,
        created_at: now.clone(),
        updated_at: now,
        started_at: None,
        completed_at: None,
    };
    (task, job)
}
