use super::*;

#[test]
fn task_ids_are_trimmed_deduplicated_and_ordered() {
    let ids = vec![
        " task-2 ".to_string(),
        "".to_string(),
        "task-1".to_string(),
        "task-2".to_string(),
    ];
    assert_eq!(normalize_task_ids(&ids), vec!["task-2", "task-1"]);
}

#[test]
fn failed_attempt_is_not_terminal_before_worker_decides_retry() {
    assert_eq!(
        visible_status(&GenerationJobState::Leased, "failed"),
        "processing"
    );
    assert_eq!(
        visible_status(&GenerationJobState::RetryWait, "pending"),
        "queued"
    );
    assert_eq!(
        visible_status(&GenerationJobState::Failed, "failed"),
        "failed"
    );
    assert_eq!(terminal_reason("queued", Some("temporary")), None);
}

#[test]
fn queue_limit_matches_aiduanju() {
    assert_eq!(MAX_ACTIVE_GENERATION_JOBS, 10_000);
}

#[tokio::test]
async fn completed_task_exposes_image_before_worker_finishes_lease() {
    let root = std::env::temp_dir().join(format!("queue-result-race-{}", Uuid::new_v4()));
    let state = crate::test_fixtures::server_state(
        root.clone(),
        mediagen_plugin_runtime::PluginLoader::empty(),
    );
    let body: OpenAiImageRequest =
        serde_json::from_value(json!({"model":"test", "prompt":"image"})).unwrap();
    let (task, mut job) = queued_image_records(
        "task-race".into(),
        "test".into(),
        &body,
        "key",
        String::new(),
        Utc::now().to_rfc3339(),
    )
    .unwrap();
    job.state = GenerationJobState::Leased;
    state.store.create_generation_job(task, job).unwrap();
    let result = json!({"created":1,"data":[{"b64_json":"image-base64"}]});
    super::super::result::save(&state, "task-race", &result)
        .await
        .unwrap();
    state
        .store
        .complete_task("task-race", json!({"image_count":1}));
    let detail = build_task_detail(&state, &owner_key("key"), "task-race", &HashMap::new())
        .await
        .unwrap();
    assert_eq!(detail["status"], "completed");
    assert_eq!(detail["result"], result);
    assert_eq!(state.store.recover_leased_generation_jobs(Utc::now()), 1);
    assert_eq!(
        state.store.generation_job("task-race", None).unwrap().state,
        GenerationJobState::Completed
    );
    assert_eq!(
        state.store.task_by_id("task-race").unwrap().status,
        "completed"
    );
    assert!(build_task_detail(
        &state,
        &owner_key("another-key"),
        "task-race",
        &HashMap::new()
    )
    .await
    .is_none());
    std::fs::remove_dir_all(root).unwrap();
}
