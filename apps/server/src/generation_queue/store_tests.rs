use chrono::{Duration, Utc};

use crate::store::{AppStore, GenerationJobState};

use super::queued_image_records;

#[test]
fn queue_orders_promoted_jobs_and_cancels_task_atomically() {
    let store = test_store("actions");
    insert_job(&store, "task-1", "owner-1");
    insert_job(&store, "task-2", "owner-1");

    store
        .apply_generation_queue_action(Some("owner-1"), "promote", &["task-2".to_string()])
        .unwrap();
    let (jobs, total) = store.list_generation_jobs("owner-1", 100, 0);
    assert_eq!(total, 2);
    assert_eq!(jobs[0].task_id, "task-2");

    store
        .apply_generation_queue_action(None, "cancel", &["task-2".to_string()])
        .unwrap();
    assert_eq!(store.task_by_id("task-2").unwrap().status, "cancelled");
    assert_eq!(
        store.generation_job("task-2", None).unwrap().state,
        GenerationJobState::Cancelled
    );
}

#[test]
fn leased_jobs_recover_after_restart_and_retry() {
    let store = test_store("recover");
    insert_job(&store, "task-recover", "owner-1");
    let now = Utc::now();
    let claimed = store
        .claim_next_generation_job(now, now + Duration::hours(6))
        .unwrap();
    assert_eq!(claimed.state, GenerationJobState::Leased);

    store.set_task_account("task-recover", "original-account");
    let path = store.path.clone();
    drop(store);
    let store = AppStore::load(path);
    assert_eq!(
        store
            .task_by_id("task-recover")
            .unwrap()
            .account_id
            .as_deref(),
        Some("original-account")
    );
    assert_eq!(store.recover_leased_generation_jobs(now), 1);
    assert_eq!(
        store.generation_job("task-recover", None).unwrap().state,
        GenerationJobState::RetryWait
    );
    assert_eq!(store.task_by_id("task-recover").unwrap().status, "pending");
}

#[test]
fn owner_cannot_modify_another_owners_queue() {
    let store = test_store("owner");
    insert_job(&store, "task-private", "owner-1");

    let error = store
        .apply_generation_queue_action(Some("owner-2"), "cancel", &["task-private".to_string()])
        .unwrap_err();

    assert!(error.contains("不存在"));
    assert_eq!(store.task_by_id("task-private").unwrap().status, "pending");
}

fn insert_job(store: &AppStore, task_id: &str, owner_key: &str) {
    let now = Utc::now().to_rfc3339();
    let body = serde_json::from_str(r#"{"model":"fake-image","prompt":"draw"}"#).unwrap();
    let (task, mut job) = queued_image_records(
        task_id.to_string(),
        "fake".to_string(),
        &body,
        "caller-key",
        "encrypted".to_string(),
        now,
    )
    .unwrap();
    job.owner_key = owner_key.to_string();
    store.create_generation_job(task, job).unwrap();
}

fn test_store(name: &str) -> AppStore {
    let root = std::env::temp_dir().join(format!(
        "mediagen-generation-queue-store-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    AppStore::load(root.join("state.json"))
}
