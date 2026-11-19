use super::*;
#[actix_web::test]
async fn accepted_task_network_failure_preserves_account_but_explicit_rejection_stops() {
    let state = web::Data::new(crate::video_api_tests::video_state(
        "failure-recovery",
        "https://example.test/video",
        0,
    ));
    let mut manifest = state
        .plugin_loader
        .get_plugin("fake-video")
        .unwrap()
        .clone();
    manifest.extra.insert("failure_strategy".into(),serde_json::json!([
        {"contains_any":["denied-test"],"category":"permission_denied","retry_action":"failover"},
        {"contains_any":["upstream_generation_failed"],"category":"upstream_failure","retry_action":"fail"}
    ]));
    let account = state.store.account_by_id("video-account").unwrap();
    let model = manifest.models[0].clone();
    let host = crate::server_plugin_host::ServerPluginHost::new_for_account(&account)
        .with_storage(
            state.credential_cipher.clone(),
            &account.profile_dir,
            &manifest,
        )
        .with_events(None, "original-task".into());
    host.checkpoint(&serde_json::json!({"value":{"phase":"accepted","task_id":"upstream"}}))
        .await
        .unwrap();
    let mut context = MediaFlowContext {
        manifest: &manifest,
        model: &model,
        account,
        task_id: "original-task".into(),
    };
    for (message, expected, detail) in [
        (
            "attachment_limit_exceeded: {\"count\":15,\"limit\":10}",
            400,
            "15 个附件",
        ),
        (
            "upstream_generation_failed: {\"message\":\"无法完成请求\"}",
            409,
            "无法完成请求",
        ),
    ] {
        let decision = classify_failure(&manifest, message);
        let error = fail_task(&state, &context, &decision).unwrap_err();
        assert_eq!(error.0, expected);
        assert!(error.2.contains(detail));
    }
    for (message, expected) in [
        ("network timeout", 503),
        ("denied-test", 403),
        (
            "upstream_generation_failed: {\"message\":\"无法完成请求\"}",
            409,
        ),
    ] {
        let mut excluded = vec![];
        let result = handle_attempt_failure(
            &state,
            &mut context,
            message,
            "视频",
            &mut 0,
            Some("video-key"),
            &mut excluded,
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
        assert_eq!(result.0, expected);
        assert!(excluded.is_empty());
        assert_eq!(context.account.id, "video-account");
        assert_eq!(
            host.checkpoint(&serde_json::json!({})).await.unwrap()["task_id"],
            "upstream"
        );
    }
}
