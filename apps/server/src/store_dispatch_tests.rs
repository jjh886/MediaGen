use std::sync::{Arc, Barrier};

use crate::store::{AppStore, TaskRecord};
use crate::test_fixtures::account_record;

#[test]
fn concurrent_dispatch_never_exceeds_account_limit() {
    let store = Arc::new(AppStore::load(test_store_path()));
    let mut account = account_record("acc-shared", "platform-a", "Shared", "");
    account.concurrency_limit = 2;
    store.insert_account(account);

    let barrier = Arc::new(Barrier::new(4));
    let workers = (0..4)
        .map(|index| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.with_account_dispatch(|| reserve_task(&store, index))
            })
        })
        .collect::<Vec<_>>();

    let reserved = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .filter(|reserved| *reserved)
        .count();
    assert_eq!(reserved, 2);
}

fn reserve_task(store: &AppStore, index: usize) -> bool {
    let Some(account) = store.first_available_account_for_key("platform-a", None) else {
        return false;
    };
    store.insert_task(TaskRecord {
        id: format!("task-{index}"),
        platform: "platform-a".to_string(),
        task_kind: "image".to_string(),
        model_id: "example-model".to_string(),
        prompt: "draw".to_string(),
        account_id: Some(account.id),
        status: "queued".to_string(),
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        created_at: "2026-07-30T00:00:00Z".to_string(),
        completed_at: None,
    });
    true
}

fn test_store_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mediagen-dispatch-{}.json", uuid::Uuid::new_v4()))
}
