use super::*;
#[actix_web::test]
async fn bound_session_never_uses_another_available_account() {
    let state = crate::protocol_api_tests::state();
    let mut other = state.store.account_by_id("account").unwrap();
    other.id = "other".into();
    state.store.insert_account(other);
    state.store.mark_account_state("account", "login_expired");
    let (manifest, _, _) = crate::model_api::resolve(&state, "text-model", "key-a").unwrap();
    let request = json!({"options":{"platform_session":{"conversation_id":"original"}}});
    for preferred in [Some("account"), Some("deleted"), None] {
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            reserve_affinity_wait(
                &state,
                &manifest,
                "text-model",
                "text",
                "key-a",
                "bound",
                Some(&request),
                preferred,
            ),
        )
        .await
        .unwrap();
        assert!(result.is_err());
        assert!(state.store.task_by_id("bound").is_none());
    }
    let (selected, _lease) = reserve_affinity(
        &state,
        &manifest,
        "text-model",
        "text",
        "key-a",
        "fresh",
        None,
        Some("account"),
    )
    .unwrap();
    assert_eq!(selected.id, "other");
}

#[actix_web::test]
async fn unbound_sync_request_uses_the_next_freed_account_instead_of_waiting_for_preference() {
    let state = crate::protocol_api_tests::state();
    state.store.update_account(
        "account",
        crate::store::AccountUpdate {
            text_concurrency_limit: Some(1),
            ..Default::default()
        },
    );
    let mut other = state.store.account_by_id("account").unwrap();
    other.id = "other".into();
    state.store.insert_account(other);
    let manifest = state.plugin_loader.get_plugin("test").unwrap();
    let (_, first) = reserve_affinity(
        &state,
        manifest,
        "text-model",
        "text",
        "key-a",
        "first",
        None,
        Some("account"),
    )
    .unwrap();
    let (_, second) = reserve_affinity(
        &state,
        manifest,
        "text-model",
        "text",
        "key-a",
        "second",
        None,
        Some("other"),
    )
    .unwrap();
    let waiting = reserve_affinity_wait(
        &state,
        manifest,
        "text-model",
        "text",
        "key-a",
        "next",
        None,
        Some("account"),
    );
    let release = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(state.store.task_by_id("next").unwrap().status, "queued");
        state.store.complete_task("second", json!({}));
        drop(second);
    };
    let (result, _) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(waiting, release)
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap().0.id, "other");
    drop(first);
}

#[actix_web::test]
async fn busy_bound_session_waits_for_original_even_when_another_is_idle() {
    let state = crate::protocol_api_tests::state();
    let mut other = state.store.account_by_id("account").unwrap();
    other.id = "other".into();
    state.store.insert_account(other);
    state.store.update_account(
        "account",
        crate::store::AccountUpdate {
            text_concurrency_limit: Some(1),
            ..Default::default()
        },
    );
    let (manifest, _, _) = crate::model_api::resolve(&state, "text-model", "key-a").unwrap();
    let (_, busy) = reserve_affinity(
        &state,
        &manifest,
        "text-model",
        "text",
        "key-a",
        "busy-bound",
        None,
        Some("account"),
    )
    .unwrap();
    let request = json!({"options":{"platform_session":{"conversation_id":"original"}}});
    let wait = reserve_affinity_wait(
        &state,
        &manifest,
        "text-model",
        "text",
        "key-a",
        "continued",
        Some(&request),
        Some("account"),
    );
    let release = async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            state.store.task_by_id("continued").unwrap().status,
            "queued"
        );
        let (account, _lease) = tokio::time::timeout(
            Duration::from_millis(100),
            reserve_affinity_wait(
                &state,
                &manifest,
                "text-model",
                "text",
                "key-a",
                "fresh-sync",
                None,
                None,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(account.id, "other");
        assert!(state.store.generation_job("fresh-sync", None).is_none());
        state.store.complete_task("busy-bound", json!({}));
        drop(busy);
    };
    let (result, _) = tokio::join!(wait, release);
    assert_eq!(result.unwrap().0.id, "account");
}

#[actix_web::test]
async fn cancelled_wait_releases_queue_and_unconfigured_accounts_do_not_wait() {
    let state = crate::protocol_api_tests::state();
    state.store.update_account(
        "account",
        crate::store::AccountUpdate {
            text_concurrency_limit: Some(1),
            ..Default::default()
        },
    );
    let (manifest, _, _) = crate::model_api::resolve(&state, "text-model", "key-a").unwrap();
    let (_, busy) = reserve_affinity(
        &state,
        &manifest,
        "text-model",
        "text",
        "key-a",
        "busy-cancel",
        None,
        None,
    )
    .unwrap();
    let waiting = reserve_affinity_wait(
        &state,
        &manifest,
        "text-model",
        "text",
        "key-a",
        "cancel-wait",
        None,
        None,
    );
    assert!(tokio::time::timeout(Duration::from_millis(25), waiting)
        .await
        .is_err());
    assert_eq!(
        state.store.task_by_id("cancel-wait").unwrap().status,
        "failed"
    );
    assert_eq!(
        state
            .store
            .list_tasks(None)
            .iter()
            .filter(|task| task.status == "queued")
            .count(),
        0
    );
    drop(busy);
    state.store.toggle_account("account", true);
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        reserve_affinity_wait(
            &state,
            &manifest,
            "text-model",
            "text",
            "key-a",
            "unconfigured",
            None,
            None,
        ),
    )
    .await
    .unwrap();
    assert!(result.is_err());
    assert!(state.store.task_by_id("unconfigured").is_none());
}
#[actix_web::test]
async fn busy_account_waits_and_dispatches_without_duplicate_records() {
    let state = crate::protocol_api_tests::state();
    let account = state.store.account_by_id("account").unwrap();
    state.store.update_account(
        &account.id,
        crate::store::AccountUpdate {
            text_concurrency_limit: Some(1),
            ..Default::default()
        },
    );
    let (manifest, _, _) = crate::model_api::resolve(&state, "text-model", "key-a").unwrap();
    let (_, lease) = reserve_affinity(
        &state,
        &manifest,
        "text-model",
        "text",
        "key-a",
        "busy",
        None,
        None,
    )
    .unwrap();
    let release = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        state
            .store
            .complete_task("busy", json!({"status":"completed"}));
        drop(lease);
    };
    let wait = reserve_affinity_wait(
        &state,
        &manifest,
        "text-model",
        "text",
        "key-a",
        "next",
        None,
        None,
    );
    let (result, _) = tokio::join!(wait, release);
    let (_, permit) = result.unwrap();
    assert_eq!(
        state
            .store
            .list_tasks(None)
            .iter()
            .filter(|task| task.id == "next")
            .count(),
        1
    );
    assert_eq!(state.store.task_by_id("next").unwrap().status, "processing");
    drop(permit);
}
