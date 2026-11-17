use super::*;
use serde_json::json;

fn task(state: &ServerState, id: &str) {
    state.store.insert_task(crate::store::TaskRecord {
        id: id.into(),
        platform: "test".into(),
        task_kind: "text".into(),
        model_id: "text-model".into(),
        prompt: String::new(),
        account_id: Some("account".into()),
        status: "processing".into(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        completed_at: None,
    });
}

async fn upload(state: &ServerState) -> String {
    let id = format!("file-{}", uuid::Uuid::new_v4());
    tokio::fs::create_dir_all(&state.file_root).await.unwrap();
    tokio::fs::write(file_api::content_path(&state.file_root, &id), b"attachment")
        .await
        .unwrap();
    file_api::write_record(
        &state.file_root,
        &file_api::FileRecord {
            id: id.clone(),
            object: "file".into(),
            bytes: 10,
            created_at: 0,
            filename: "test.txt".into(),
            purpose: "user_data".into(),
            mime_type: "text/plain".into(),
            owner_hash: crate::access_control::hash_secret("key-a"),
            used_by: Vec::new(),
        },
    )
    .await
    .unwrap();
    id
}

#[actix_web::test]
async fn shared_upload_survives_running_request_and_retry_then_cleans_both_copies() {
    let state = crate::protocol_api_tests::state();
    let id = upload(&state).await;
    let input = json!({"input":[{"content":[{"file_id":id}]}]});
    assert!(register(&state, "intruder", "key-b", &input).await.is_err());
    task(&state, "first");
    task(&state, "second");
    let first = register(&state, "first", "key-a", &input).await.unwrap();
    let second = register(&state, "second", "key-a", &input).await.unwrap();
    let account = state.store.list_accounts(None).remove(0);
    let workspace =
        crate::server_plugin_host::recovery_workspace(&account.profile_dir, "first").unwrap();
    tokio::fs::write(workspace.join("attachment.bin"), b"copy")
        .await
        .unwrap();
    state
        .store
        .complete_task("first", json!({"status":"completed"}));
    sweep(&state).await.unwrap();
    assert!(workspace.exists());
    drop(first);
    sweep(&state).await.unwrap();
    assert!(!workspace.exists());
    assert!(file_api::content_path(&state.file_root, &id).exists());
    state
        .store
        .fail_task("second", "temporary_unavailable: reconnect".into());
    drop(second);
    sweep(&state).await.unwrap();
    assert!(file_api::content_path(&state.file_root, &id).exists());
    state
        .store
        .fail_task("second", "generation_failed: stopped".into());
    sweep(&state).await.unwrap();
    assert!(!file_api::content_path(&state.file_root, &id).exists());
    assert!(!file_api::metadata_path(&state.file_root, &id).exists());
    sweep(&state).await.unwrap();
}

#[actix_web::test]
async fn saved_history_survives_until_session_deleted_and_unused_upload_is_kept() {
    let state = crate::protocol_api_tests::state();
    let id = upload(&state).await;
    let unused = upload(&state).await;
    task(&state, "saved");
    drop(
        register(
            &state,
            "saved",
            "key-a",
            &json!({"input":[{"content":[{"file_id":id}]}]}),
        )
        .await
        .unwrap(),
    );
    state
        .store
        .complete_task("saved", json!({"response":{"id":"resp_saved"}}));
    sweep(&state).await.unwrap();
    assert!(file_api::content_path(&state.file_root, &id).exists());
    state
        .store
        .complete_task("saved", json!({"status":"deleted"}));
    sweep(&state).await.unwrap();
    assert!(!file_api::content_path(&state.file_root, &id).exists());
    assert!(file_api::content_path(&state.file_root, &unused).exists());
}

#[actix_web::test]
async fn native_session_continues_without_resolving_deleted_attachment() {
    let state = crate::protocol_api_tests::state();
    let id = upload(&state).await;
    task(&state, "native");
    drop(
        register(
            &state,
            "native",
            "key-a",
            &json!({"input":[{"content":[{"file_id":id}]}]}),
        )
        .await
        .unwrap(),
    );
    let response =
        json!({"output_text":"answer","metadata":{"platform_session":{"id":"upstream"}}});
    crate::conversation_affinity::write_session(
        &state, "session", "account", "native", None, &response,
    )
    .await
    .unwrap();
    state.store.complete_task(
        "native",
        json!({"conversation_session_id":"session","response":response}),
    );
    sweep(&state).await.unwrap();
    assert!(!file_api::content_path(&state.file_root, &id).exists());
    let mut input = json!({"input":[
        {"role":"system","content":"instructions"},
        {"role":"user","content":[{"type":"input_file","file_id":id}]},
        {"role":"assistant","content":"answer"},
        {"role":"user","content":"continue"}
    ]});
    let saved = crate::conversation_affinity::read_session(&state, "session")
        .unwrap()
        .unwrap();
    crate::conversation_affinity::apply_cursor(&mut input, &saved);
    crate::protocol_files::retain_current_input(&mut input);
    assert_eq!(input["input"].as_array().unwrap().len(), 2);
    assert_eq!(input["input"][0]["role"], "system");
    assert_eq!(input["options"]["session_input_start"], 1);
    crate::protocol_files::resolve(&state, "key-a", &mut input, &json!({}))
        .await
        .unwrap();
}

#[actix_web::test]
async fn queued_upload_is_registered_before_dispatch_and_freed_after_cancellation() {
    use actix_web::{test, App};
    let state = crate::protocol_api_tests::state();
    let id = upload(&state).await;
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let response = test::call_service(&app, test::TestRequest::post().uri("/v1/responses")
        .insert_header(("authorization", "Bearer key-a"))
        .set_json(json!({"model":"text-model","async":true,"input":[{"role":"user","content":[{"type":"input_file","file_id":id}]}]})).to_request()).await;
    assert_eq!(response.status(), 202);
    let body: Value = test::read_body_json(response).await;
    let task = body["data"]["taskId"].as_str().unwrap();
    // 排队任务的业务记录可能暂时标记失败，调度状态仍然必须保护附件。
    state
        .store
        .fail_task(task, "generation_failed: previous attempt".into());
    sweep(&state).await.unwrap();
    assert!(file_api::content_path(&state.file_root, &id).exists());
    state
        .store
        .finish_generation_job(task, GenerationJobState::Cancelled, Some("cancelled"));
    sweep(&state).await.unwrap();
    assert!(!file_api::content_path(&state.file_root, &id).exists());
}
