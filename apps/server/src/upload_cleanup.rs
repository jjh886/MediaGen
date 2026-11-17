use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Mutex,
    time::Duration,
};

use actix_web::web;
use serde_json::Value;

use crate::{file_api, store::GenerationJobState, ServerState};

// 登记与删除串行，执行中的请求另外持有引用，避免完成状态重放时误删文件。
static FILES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static ACTIVE: Mutex<Option<HashMap<(PathBuf, String), usize>>> = Mutex::new(None);

pub(crate) struct UploadUse(PathBuf, String);

impl Drop for UploadUse {
    fn drop(&mut self) {
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(count) = active
            .as_mut()
            .and_then(|v| v.get_mut(&(self.0.clone(), self.1.clone())))
        {
            *count -= 1;
            if *count == 0 {
                active
                    .as_mut()
                    .unwrap()
                    .remove(&(self.0.clone(), self.1.clone()));
            }
        }
    }
}

fn active(scope: &std::path::Path, task: &str) -> UploadUse {
    let guard = UploadUse(scope.to_owned(), task.into());
    *ACTIVE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .entry((guard.0.clone(), guard.1.clone()))
        .or_default() += 1;
    guard
}

fn busy(scope: &std::path::Path, task: &str) -> bool {
    ACTIVE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|values| values.contains_key(&(scope.to_owned(), task.into())))
}

pub(crate) async fn workspace_use(profile: &str, task: &str) -> UploadUse {
    let _lock = FILES.lock().await;
    active(std::path::Path::new(profile), task)
}

pub(crate) async fn register(
    state: &ServerState,
    task: &str,
    key: &str,
    request: &Value,
) -> Result<UploadUse, String> {
    let _lock = FILES.lock().await;
    let guard = active(&state.file_root, task);
    // 已完成请求由检查点重放，不再读取已释放的上传原件。
    if state
        .store
        .task_by_id(task)
        .is_some_and(|v| v.status == "completed")
    {
        return Ok(guard);
    }
    let owner = crate::access_control::hash_secret(key);
    let mut ids = Vec::new();
    file_ids(request, &mut ids);
    ids.sort();
    ids.dedup();
    for id in ids {
        let mut record = file_api::read_record_for_owner(&state.file_root, &id, &owner).await?;
        if !record.used_by.iter().any(|value| value == task) {
            record.used_by.push(task.into());
            file_api::write_record(&state.file_root, &record).await?;
        }
    }
    Ok(guard)
}

fn file_ids(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            if let Some(id) = fields.get("file_id").and_then(Value::as_str) {
                ids.push(id.into());
            }
            for name in [
                "input",
                "messages",
                "content",
                "file",
                "images",
                "mask",
                "input_reference",
                "input_references",
                "video_reference",
                "video_references",
                "audio_reference",
                "audio_references",
            ] {
                if let Some(child) = fields.get(name) {
                    file_ids(child, ids);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                file_ids(child, ids);
            }
        }
        _ => {}
    }
}

fn terminal(state: &ServerState, id: &str) -> bool {
    if let Some(job) = state
        .store
        .generation_jobs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|job| job.task_id == id)
    {
        return matches!(
            job.state,
            GenerationJobState::Completed
                | GenerationJobState::Failed
                | GenerationJobState::Cancelled
        );
    }
    state
        .store
        .tasks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|task| task.id == id)
        .is_some_and(|task| {
            matches!(task.status.as_str(), "completed" | "cancelled")
                || (task.status == "failed"
                    && task.error_message.as_deref().is_some_and(|error| {
                        matches!(
                            error.split(':').next().unwrap_or(""),
                            "insufficient_quota"
                                | "no_available_account_for_model"
                                | "conversation_unavailable"
                                | "recovery_exhausted"
                                | "generation_failed"
                                | "proxy_unavailable"
                        )
                    }))
        })
}

fn needs_history(state: &ServerState, id: &str) -> bool {
    let Some(value) = state
        .store
        .task_by_id(id)
        .and_then(|v| v.result_data)
        .and_then(|v| serde_json::from_str::<Value>(&v).ok())
    else {
        return false;
    };
    if value["response"].is_object() {
        return true;
    }
    value["conversation_session_id"]
        .as_str()
        .is_some_and(|session| {
            crate::conversation_affinity::read_session(state, session)
                .map_or(true, |saved| saved.is_some())
        })
}

pub(crate) async fn sweep(state: &ServerState) -> std::io::Result<()> {
    let mut entries = match tokio::fs::read_dir(&state.file_root).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json")
            || !entry.file_type().await?.is_file()
        {
            continue;
        }
        let _lock = FILES.lock().await;
        let mut record = match file_api::read_record_path(&path).await {
            Ok(record) if file_api::metadata_path(&state.file_root, &record.id) == path => record,
            _ => continue,
        };
        let previous_count = record.used_by.len();
        record
            .used_by
            .retain(|id| busy(&state.file_root, id) || state.store.task_by_id(id).is_some());
        if previous_count != record.used_by.len() {
            file_api::write_record(&state.file_root, &record)
                .await
                .map_err(std::io::Error::other)?;
        }
        if record.used_by.is_empty()
            || record.used_by.iter().any(|id| {
                busy(&state.file_root, id) || !terminal(state, id) || needs_history(state, id)
            })
        {
            continue;
        }
        remove_file(&file_api::content_path(&state.file_root, &record.id)).await?;
        remove_file(&path).await?;
        eprintln!(
            "{}",
            serde_json::json!({"event":"upload_cleaned","file_id":record.id,"bytes":record.bytes})
        );
    }
    let tasks = state
        .store
        .tasks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|task| {
            (
                crate::access_control::hash_secret(&task.id),
                task.id.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let profiles = state
        .store
        .list_accounts(None)
        .into_iter()
        .map(|account| account.profile_dir)
        .collect::<HashSet<_>>();
    for profile in profiles {
        if profile.is_empty() {
            continue;
        }
        let root = PathBuf::from(&profile);
        let mut entries = match tokio::fs::read_dir(root.join("workspaces")).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let Some(task) = name.to_str().and_then(|name| tasks.get(name)) else {
                continue;
            };
            let _lock = FILES.lock().await;
            if busy(&state.file_root, task)
                || busy(&root, task)
                || !terminal(state, task)
                || !entry.file_type().await?.is_dir()
            {
                continue;
            }
            match tokio::fs::remove_dir_all(entry.path()).await {
                Ok(()) => eprintln!(
                    "{}",
                    serde_json::json!({"event":"upload_workspace_cleaned","task_id":task})
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

async fn remove_file(path: &std::path::Path) -> std::io::Result<()> {
    match tokio::fs::remove_file(path).await {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub(crate) fn start_worker(state: web::Data<ServerState>) {
    actix_web::rt::spawn(async move {
        loop {
            if let Err(error) = sweep(&state).await {
                eprintln!(
                    "{}",
                    serde_json::json!({"event":"upload_cleanup_failed","error":error.to_string(),"retry_seconds":10})
                );
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

#[cfg(test)]
#[path = "upload_cleanup_tests.rs"]
mod tests;
