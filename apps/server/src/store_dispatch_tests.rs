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

#[test]
fn plugin_defaults_control_text_capacity_and_preserve_account_overrides_after_reload() {
    let root = std::env::temp_dir().join(format!("plugin-defaults-{}", uuid::Uuid::new_v4()));
    for (platform, limit) in [("unlimited", 0), ("limited-concurrency", 3)] {
        let dir = root.join(platform);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), serde_json::json!({
            "platform_id":platform, "display_name":platform,
            "tiers":{"free":{"display_name":"Default","is_default":true,"text_concurrency_limit":limit}}
        }).to_string()).unwrap();
    }
    let plugins = mediagen_plugin_runtime::PluginLoader::load_from(&root).unwrap();
    for (platform, limit) in [("unlimited", 0), ("limited-concurrency", 3), ("unknown", 0)] {
        let path = test_store_path();
        let store = AppStore::load(path.clone());
        let mut account = account_record("account", platform, "默认账号", "");
        // 旧账号缺失字段与显式填 0 均使用插件默认；默认值不写入快照。
        let mut legacy = serde_json::to_value(&account).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("text_concurrency_limit");
        assert!(legacy.get("text_concurrency_default").is_none());
        account = serde_json::from_value(legacy).unwrap();
        assert_eq!(account.policy.text_concurrency_limit, 0);
        store.insert_account(account);
        store.apply_plugin_defaults(&plugins);
        let configured = store.account_by_id("account").unwrap();
        assert_eq!(configured.policy.text_concurrency_default, limit);
        assert_eq!(
            crate::routes::public_account(configured)["text_concurrency_default"],
            limit
        );
        for index in 0..if limit == 0 { 105 } else { limit } {
            let selected = store
                .select_account(platform, None, &[], "text")
                .expect("默认并发内应可调度");
            store.insert_task(TaskRecord {
                id: format!("text-{index}"),
                platform: platform.into(),
                task_kind: "text".into(),
                model_id: "text".into(),
                prompt: String::new(),
                account_id: Some(selected.id),
                status: "processing".into(),
                result_url: None,
                storage_key: None,
                error_message: None,
                result_data: None,
                created_at: chrono::Utc::now().to_rfc3339(),
                completed_at: None,
            });
        }
        assert_eq!(
            store.select_account(platform, None, &[], "text").is_some(),
            limit == 0
        );
        assert!(store.select_account(platform, None, &[], "image").is_some());
        if limit == 0 {
            store.update_account(
                "account",
                crate::store::AccountUpdate {
                    text_daily_limit: Some(105),
                    ..Default::default()
                },
            );
            assert!(store.select_account(platform, None, &[], "text").is_none());
            store.update_account(
                "account",
                crate::store::AccountUpdate {
                    text_daily_limit: Some(0),
                    ..Default::default()
                },
            );
            assert!(store.select_account(platform, None, &[], "text").is_some());
        }
        store.update_account(
            "account",
            crate::store::AccountUpdate {
                text_concurrency_limit: Some(150),
                ..Default::default()
            },
        );
        assert!(store.select_account(platform, None, &[], "text").is_some());
        // 账号显式配置（包括旧值 3）不能在重启或载入插件时被覆盖。
        store.update_account(
            "account",
            crate::store::AccountUpdate {
                text_concurrency_limit: Some(3),
                ..Default::default()
            },
        );
        let reloaded = AppStore::load(path);
        reloaded.apply_plugin_defaults(&plugins);
        let account = reloaded.account_by_id("account").unwrap();
        assert_eq!(crate::store_helpers::text_concurrency_limit(&account), 3);
        let account = reloaded
            .update_account(
                "account",
                crate::store::AccountUpdate {
                    text_concurrency_limit: Some(0),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            crate::store_helpers::text_concurrency_limit(&account),
            limit
        );
    }
}
