#[cfg(test)]
#[path = "test_helpers.rs"]
mod test_helpers;

#[cfg(test)]
mod tests {
    use super::test_helpers::{make_storage, sample_account, sample_task};
    use crate::types::{LoginState, TaskKind, TaskStatus};
    use chrono::{TimeZone, Utc};

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
    fn list_by_platform_refreshes_stale_daily_usage() {
        let (db, _dir) = make_storage();
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let mut acc = sample_account("a1", "doubao");
        acc.login_state = LoginState::DailyQuotaExhausted;
        acc.login_state_reason = "当日用量已达上限".to_string();
        acc.used_today = 8;
        acc.used_total = 42;
        acc.usage_date = "2026-04-13".to_string();
        db.insert_account(&acc).unwrap();

        let accounts = db.list_accounts_by_platform("doubao").unwrap();

        assert_eq!(accounts[0].used_today, 0);
        assert_eq!(accounts[0].used_total, 42);
        assert_eq!(accounts[0].usage_date, today);
        assert_eq!(accounts[0].login_state, LoginState::Available);
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
