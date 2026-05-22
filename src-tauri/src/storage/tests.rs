#[cfg(test)]
mod tests {
    use crate::storage::Storage;
    use crate::types::{LoginState, PlatformAccount, TaskKind, TaskRecord, TaskStatus};
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    fn make_storage() -> (Storage, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = Storage::new(&path).unwrap();
        (storage, dir)
    }

    fn sample_account(id: &str, platform: &str) -> PlatformAccount {
        let now = Utc::now();
        PlatformAccount {
            id: id.to_string(),
            platform: platform.to_string(),
            name: "测试账号".to_string(),
            notes: String::new(),
            login_state: LoginState::Available,
            login_state_reason: String::new(),
            login_state_updated_at: now,
            cookies: String::new(),
            api_token: "token123".to_string(),
            refresh_token: String::new(),
            token_expires_at: None,
            profile_dir: String::new(),
            tier: "free".to_string(),
            concurrency_limit: 1,
            daily_limit: 100,
            total_limit: 1000,
            used_today: 0,
            used_total: 0,
            usage_date: "2026-04-13".to_string(),
            cooldown_until: None,
            user_id: String::new(),
            username: "tester".to_string(),
            email: "test@example.com".to_string(),
            avatar_url: String::new(),
            password_encrypted: String::new(),
            created_at: now,
            last_check_at: None,
            last_refreshed_at: None,
            last_used_at: None,
        }
    }

    fn sample_task(id: &str) -> TaskRecord {
        TaskRecord {
            id: id.to_string(),
            platform: "doubao".to_string(),
            task_kind: TaskKind::Image,
            model_id: "model-v1".to_string(),
            prompt: "一只猫".to_string(),
            account_id: Some("acc-1".to_string()),
            status: TaskStatus::Pending,
            result_url: None,
            storage_key: None,
            error_message: None,
            result_data: None,
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    #[test]
    fn insert_and_get_account() {
        let (db, _dir) = make_storage();
        let acc = sample_account("a1", "doubao");
        db.insert_account(&acc).unwrap();

        let loaded = db.get_account("a1").unwrap().expect("should exist");
        assert_eq!(loaded.id, "a1");
        assert_eq!(loaded.platform, "doubao");
        assert_eq!(loaded.login_state, LoginState::Available);
        assert_eq!(loaded.api_token, "token123");
    }

    #[test]
    fn list_by_platform() {
        let (db, _dir) = make_storage();
        db.insert_account(&sample_account("a1", "doubao")).unwrap();
        db.insert_account(&sample_account("a2", "doubao")).unwrap();
        db.insert_account(&sample_account("a3", "kling")).unwrap();

        let doubao = db.list_accounts_by_platform("doubao").unwrap();
        assert_eq!(doubao.len(), 2);
        let kling = db.list_accounts_by_platform("kling").unwrap();
        assert_eq!(kling.len(), 1);
    }

    #[test]
    fn list_by_platform_has_stable_order() {
        let (db, _dir) = make_storage();
        let newer = Utc.with_ymd_and_hms(2026, 4, 24, 10, 0, 0).unwrap();
        let older = Utc.with_ymd_and_hms(2026, 4, 23, 10, 0, 0).unwrap();

        let mut b = sample_account("b", "doubao");
        b.created_at = newer;
        let mut a = sample_account("a", "doubao");
        a.created_at = newer;
        let mut old = sample_account("old", "doubao");
        old.created_at = older;

        db.insert_account(&b).unwrap();
        db.insert_account(&old).unwrap();
        db.insert_account(&a).unwrap();

        let ids: Vec<String> = db
            .list_accounts_by_platform("doubao")
            .unwrap()
            .into_iter()
            .map(|account| account.id)
            .collect();

        assert_eq!(ids, vec!["a", "b", "old"]);
    }

    #[test]
    fn update_login_state() {
        let (db, _dir) = make_storage();
        db.insert_account(&sample_account("a1", "doubao")).unwrap();
        db.update_login_state("a1", LoginState::LoginExpired, "cookie过期")
            .unwrap();

        let acc = db.get_account("a1").unwrap().unwrap();
        assert_eq!(acc.login_state, LoginState::LoginExpired);
        assert_eq!(acc.login_state_reason, "cookie过期");
    }

    #[test]
    fn update_usage_and_reset() {
        let (db, _dir) = make_storage();
        db.insert_account(&sample_account("a1", "doubao")).unwrap();
        db.update_usage("a1", 10, 50).unwrap();

        let acc = db.get_account("a1").unwrap().unwrap();
        assert_eq!(acc.used_today, 10);
        assert_eq!(acc.used_total, 50);

        let reset = db.reset_daily_usage("2026-04-14").unwrap();
        assert_eq!(reset, 1);

        let acc = db.get_account("a1").unwrap().unwrap();
        assert_eq!(acc.used_today, 0);
        assert_eq!(acc.usage_date, "2026-04-14");
    }

    #[test]
    fn delete_account() {
        let (db, _dir) = make_storage();
        db.insert_account(&sample_account("a1", "doubao")).unwrap();
        db.delete_account("a1").unwrap();
        assert!(db.get_account("a1").unwrap().is_none());
    }

    #[test]
    fn insert_and_get_task() {
        let (db, _dir) = make_storage();
        let task = sample_task("t1");
        db.insert_task(&task).unwrap();

        let loaded = db.get_task("t1").unwrap().expect("should exist");
        assert_eq!(loaded.id, "t1");
        assert_eq!(loaded.task_kind, TaskKind::Image);
        assert_eq!(loaded.status, TaskStatus::Pending);
    }

    #[test]
    fn list_tasks_pagination() {
        let (db, _dir) = make_storage();
        for i in 0..5 {
            db.insert_task(&sample_task(&format!("t{i}"))).unwrap();
        }
        let page = db.list_tasks("", 3, 0).unwrap();
        assert_eq!(page.len(), 3);
        let page2 = db.list_tasks("", 3, 3).unwrap();
        assert_eq!(page2.len(), 2);
    }

    #[test]
    fn list_tasks_has_stable_order() {
        let (db, _dir) = make_storage();
        let newer = Utc.with_ymd_and_hms(2026, 4, 24, 10, 0, 0).unwrap();
        let older = Utc.with_ymd_and_hms(2026, 4, 23, 10, 0, 0).unwrap();

        let mut b = sample_task("task-b");
        b.created_at = newer;
        let mut a = sample_task("task-a");
        a.created_at = newer;
        let mut old = sample_task("task-old");
        old.created_at = older;

        db.insert_task(&b).unwrap();
        db.insert_task(&old).unwrap();
        db.insert_task(&a).unwrap();

        let ids: Vec<String> = db
            .list_tasks("", 10, 0)
            .unwrap()
            .into_iter()
            .map(|task| task.id)
            .collect();

        assert_eq!(ids, vec!["task-a", "task-b", "task-old"]);
    }

    #[test]
    fn update_task_status() {
        let (db, _dir) = make_storage();
        db.insert_task(&sample_task("t1")).unwrap();
        db.update_task_status(
            "t1",
            TaskStatus::Completed,
            Some("https://cdn/img.png"),
            None,
        )
        .unwrap();

        let t = db.get_task("t1").unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Completed);
        assert_eq!(t.result_url.as_deref(), Some("https://cdn/img.png"));
        assert!(t.completed_at.is_some());
    }

    #[test]
    fn settings_crud() {
        let (db, _dir) = make_storage();
        assert!(db.get_setting("theme").unwrap().is_none());

        db.set_setting("theme", "dark").unwrap();
        assert_eq!(db.get_setting("theme").unwrap().as_deref(), Some("dark"));

        db.set_setting("theme", "light").unwrap();
        assert_eq!(db.get_setting("theme").unwrap().as_deref(), Some("light"));
    }

    #[test]
    fn full_account_update() {
        let (db, _dir) = make_storage();
        let mut acc = sample_account("a1", "doubao");
        db.insert_account(&acc).unwrap();

        acc.name = "新名称".to_string();
        acc.tier = "pro".to_string();
        db.update_account(&acc).unwrap();

        let loaded = db.get_account("a1").unwrap().unwrap();
        assert_eq!(loaded.name, "新名称");
        assert_eq!(loaded.tier, "pro");
    }
}

#[cfg(test)]
#[path = "tests_extended.rs"]
mod tests_extended;
