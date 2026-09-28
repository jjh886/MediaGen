use serde_json::json;

use super::{account, store_path, temp_store};
use crate::store::{AppStore, TaskRecord};

#[test]
fn task_lifecycle_updates_result_error_and_sort_order() {
    let store = temp_store("tasks");
    store.insert_task(task("task-old", "running", "2026-06-28T00:00:00Z"));
    store.insert_task(task("task-new", "running", "2026-06-29T00:00:00Z"));

    let tasks = store.list_tasks(Some("platform-a"));
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["task-new", "task-old",]
    );

    let completed = store
        .complete_task(
            "task-old",
            json!({ "images": [{ "url": "https://example.test/out.png" }] }),
        )
        .unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(
        completed.result_url.as_deref(),
        Some("https://example.test/out.png")
    );

    assert_eq!(
        store
            .fail_task("task-new", "boom".to_string())
            .unwrap()
            .error_message
            .as_deref(),
        Some("boom")
    );
    store.insert_task(task("task-cancel", "running", "2026-06-30T00:00:00Z"));
    assert_eq!(
        store.cancel_task("task-cancel").unwrap().status.as_str(),
        "cancelled"
    );
    assert_eq!(
        store
            .list_tasks(Some("platform-a"))
            .into_iter()
            .find(|task| task.id == "task-cancel")
            .unwrap()
            .error_message
            .as_deref(),
        Some("用户取消")
    );
    assert!(store.complete_task("missing", json!({})).is_none());
}

#[test]
fn task_runtime_events_usage_and_verification_are_recorded() {
    let store = temp_store("task-runtime");
    store.insert_account(account("acc-runtime", ""));
    store.insert_task(TaskRecord {
        account_id: Some("acc-runtime".to_string()),
        ..task("task-runtime", "processing", "2026-06-30T00:00:00Z")
    });

    store.add_task_event("task-runtime", "info", "开始执行".to_string());
    store.mark_account_usage("acc-runtime");
    let account = store.list_accounts(Some("platform-a")).remove(0);
    assert_eq!(account.used_today, 1);
    assert_eq!(account.used_total, 1);

    let verification = store
        .require_task_verification("task-runtime", "需要验证".to_string())
        .unwrap();
    assert_eq!(verification.status, "verification_required");
    assert_eq!(verification.error_message.as_deref(), Some("需要验证"));

    let resumed = store
        .resume_task_after_verification("task-runtime")
        .unwrap();
    assert_eq!(resumed.status, "queued");
    assert!(resumed.error_message.is_none());

    let reloaded = AppStore::load(store_path("task-runtime"));
    let interrupted = &reloaded.list_tasks(Some("platform-a"))[0];
    // 同步任务没有持久化 worker，服务重启后不能继续执行，必须结束并释放账号并发。
    assert_eq!(interrupted.status, "failed");
    assert_eq!(
        interrupted.error_message.as_deref(),
        Some("MediaGen 上次退出时任务仍在执行，请重新提交")
    );
}

fn task(id: &str, status: &str, created_at: &str) -> TaskRecord {
    TaskRecord {
        id: id.to_string(),
        platform: "platform-a".to_string(),
        task_kind: "image".to_string(),
        model_id: "image-model".to_string(),
        prompt: "draw".to_string(),
        account_id: None,
        status: status.to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: created_at.to_string(),
        completed_at: None,
    }
}
