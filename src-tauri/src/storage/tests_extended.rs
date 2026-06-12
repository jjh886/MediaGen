use super::test_helpers::{make_storage, sample_account, sample_task};
use crate::types::TaskStatus;
use chrono::Utc;

// ── 账号补充测试 ──

#[test]
fn list_all_accounts() {
    let (db, _dir) = make_storage();
    db.insert_account(&sample_account("a1", "doubao")).unwrap();
    db.insert_account(&sample_account("a2", "flow")).unwrap();

    let all = db.list_all_accounts().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn list_all_accounts_empty() {
    let (db, _dir) = make_storage();
    let all = db.list_all_accounts().unwrap();
    assert!(all.is_empty());
}

#[test]
fn list_distinct_platforms() {
    let (db, _dir) = make_storage();
    db.insert_account(&sample_account("a1", "doubao")).unwrap();
    db.insert_account(&sample_account("a2", "doubao")).unwrap();
    db.insert_account(&sample_account("a3", "flow")).unwrap();

    let platforms = db.list_distinct_platforms().unwrap();
    assert_eq!(platforms.len(), 2);
    assert_eq!(platforms[0], "doubao");
    assert_eq!(platforms[1], "flow");
}

#[test]
fn list_distinct_platforms_empty() {
    let (db, _dir) = make_storage();
    let platforms = db.list_distinct_platforms().unwrap();
    assert!(platforms.is_empty());
}

#[test]
fn update_last_check_at() {
    let (db, _dir) = make_storage();
    db.insert_account(&sample_account("a1", "doubao")).unwrap();

    let acc = db.get_account("a1").unwrap().unwrap();
    assert!(acc.last_check_at.is_none());

    db.update_last_check_at("a1").unwrap();

    let acc = db.get_account("a1").unwrap().unwrap();
    assert!(acc.last_check_at.is_some());
}

#[test]
fn get_nonexistent_account() {
    let (db, _dir) = make_storage();
    let result = db.get_account("nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn list_accounts_by_platform_empty() {
    let (db, _dir) = make_storage();
    let result = db.list_accounts_by_platform("nonexistent").unwrap();
    assert!(result.is_empty());
}

// ── 任务补充测试 ──

#[test]
fn insert_task_with_all_fields() {
    let (db, _dir) = make_storage();
    let mut task = sample_task("t1");
    task.result_url = Some("https://cdn/img.png".to_string());
    task.storage_key = Some("key-123".to_string());
    task.error_message = Some("some error".to_string());
    task.result_data = Some(r#"{"url":"https://cdn/img.png","width":1024}"#.to_string());
    task.status = TaskStatus::Completed;
    task.completed_at = Some(Utc::now());

    db.insert_task(&task).unwrap();

    let loaded = db.get_task("t1").unwrap().unwrap();
    assert_eq!(loaded.result_url.as_deref(), Some("https://cdn/img.png"));
    assert_eq!(loaded.storage_key.as_deref(), Some("key-123"));
    assert_eq!(loaded.error_message.as_deref(), Some("some error"));
    assert!(loaded.result_data.is_some());
    assert_eq!(loaded.status, TaskStatus::Completed);
    assert!(loaded.completed_at.is_some());
}

#[test]
fn list_tasks_by_platform() {
    let (db, _dir) = make_storage();
    let mut t1 = sample_task("t1");
    t1.platform = "doubao".to_string();
    let mut t2 = sample_task("t2");
    t2.platform = "flow".to_string();
    let mut t3 = sample_task("t3");
    t3.platform = "doubao".to_string();

    db.insert_task(&t1).unwrap();
    db.insert_task(&t2).unwrap();
    db.insert_task(&t3).unwrap();

    let doubao_tasks = db.list_tasks("doubao", 10, 0).unwrap();
    assert_eq!(doubao_tasks.len(), 2);
    for t in &doubao_tasks {
        assert_eq!(t.platform, "doubao");
    }

    let flow_tasks = db.list_tasks("flow", 10, 0).unwrap();
    assert_eq!(flow_tasks.len(), 1);
    assert_eq!(flow_tasks[0].platform, "flow");
}

#[test]
fn list_tasks_empty_platform_returns_all() {
    let (db, _dir) = make_storage();
    let mut t1 = sample_task("t1");
    t1.platform = "doubao".to_string();
    let mut t2 = sample_task("t2");
    t2.platform = "flow".to_string();

    db.insert_task(&t1).unwrap();
    db.insert_task(&t2).unwrap();

    let all = db.list_tasks("", 10, 0).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn update_task_result_data() {
    let (db, _dir) = make_storage();
    db.insert_task(&sample_task("t1")).unwrap();

    let t = db.get_task("t1").unwrap().unwrap();
    assert!(t.result_data.is_none());

    let data = r#"{"url":"https://cdn/img.png","width":1024,"height":768}"#;
    db.update_task_result_data("t1", data).unwrap();

    let t = db.get_task("t1").unwrap().unwrap();
    assert_eq!(t.result_data.as_deref(), Some(data));
}

#[test]
fn update_task_status_failed() {
    let (db, _dir) = make_storage();
    db.insert_task(&sample_task("t1")).unwrap();

    db.update_task_status("t1", TaskStatus::Failed, None, Some("timeout"))
        .unwrap();

    let t = db.get_task("t1").unwrap().unwrap();
    assert_eq!(t.status, TaskStatus::Failed);
    assert_eq!(t.error_message.as_deref(), Some("timeout"));
    assert!(t.completed_at.is_some());
}

#[test]
fn update_task_status_processing_no_completed_at() {
    let (db, _dir) = make_storage();
    db.insert_task(&sample_task("t1")).unwrap();

    db.update_task_status("t1", TaskStatus::Processing, None, None)
        .unwrap();

    let t = db.get_task("t1").unwrap().unwrap();
    assert_eq!(t.status, TaskStatus::Processing);
    assert!(t.completed_at.is_none());
}

#[test]
fn get_nonexistent_task() {
    let (db, _dir) = make_storage();
    let result = db.get_task("nonexistent").unwrap();
    assert!(result.is_none());
}

// ── 日志测试 ──

#[test]
fn insert_and_list_logs() {
    let (db, _dir) = make_storage();
    db.insert_log(
        "2026-04-15T10:00:00Z",
        "engine",
        "info",
        "任务开始",
        None,
        None,
    )
    .unwrap();
    db.insert_log(
        "2026-04-15T10:01:00Z",
        "scheduler",
        "error",
        "调度失败",
        Some("详细错误信息"),
        Some("task-1"),
    )
    .unwrap();

    let logs = db.list_logs("", "", "", 10, 0).unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].module, "scheduler");
    assert_eq!(logs[0].level, "error");
    assert_eq!(logs[0].detail.as_deref(), Some("详细错误信息"));
    assert_eq!(logs[0].task_id.as_deref(), Some("task-1"));
    assert_eq!(logs[1].module, "engine");
}

#[test]
fn list_logs_filter_by_module() {
    let (db, _dir) = make_storage();
    db.insert_log("2026-04-15T10:00:00Z", "engine", "info", "msg1", None, None)
        .unwrap();
    db.insert_log(
        "2026-04-15T10:01:00Z",
        "scheduler",
        "info",
        "msg2",
        None,
        None,
    )
    .unwrap();

    let logs = db.list_logs("engine", "", "", 10, 0).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].module, "engine");
}

#[test]
fn list_logs_filter_by_level() {
    let (db, _dir) = make_storage();
    db.insert_log("2026-04-15T10:00:00Z", "engine", "info", "msg1", None, None)
        .unwrap();
    db.insert_log(
        "2026-04-15T10:01:00Z",
        "engine",
        "error",
        "msg2",
        None,
        None,
    )
    .unwrap();

    let logs = db.list_logs("", "error", "", 10, 0).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].level, "error");
}

#[test]
fn list_logs_filter_by_keyword() {
    let (db, _dir) = make_storage();
    db.insert_log(
        "2026-04-15T10:00:00Z",
        "engine",
        "info",
        "任务开始生成",
        None,
        None,
    )
    .unwrap();
    db.insert_log(
        "2026-04-15T10:01:00Z",
        "engine",
        "info",
        "调度完成",
        None,
        None,
    )
    .unwrap();

    let logs = db.list_logs("", "", "生成", 10, 0).unwrap();
    assert_eq!(logs.len(), 1);
    assert!(logs[0].summary.contains("生成"));
}

#[test]
fn list_logs_combined_filters() {
    let (db, _dir) = make_storage();
    db.insert_log(
        "2026-04-15T10:00:00Z",
        "engine",
        "error",
        "引擎错误",
        None,
        None,
    )
    .unwrap();
    db.insert_log(
        "2026-04-15T10:01:00Z",
        "engine",
        "info",
        "引擎信息",
        None,
        None,
    )
    .unwrap();
    db.insert_log(
        "2026-04-15T10:02:00Z",
        "scheduler",
        "error",
        "调度错误",
        None,
        None,
    )
    .unwrap();

    let logs = db.list_logs("engine", "error", "引擎", 10, 0).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].summary, "引擎错误");
}

#[test]
fn list_logs_pagination() {
    let (db, _dir) = make_storage();
    for i in 0..5 {
        db.insert_log(
            &format!("2026-04-15T10:0{i}:00Z"),
            "engine",
            "info",
            &format!("msg{i}"),
            None,
            None,
        )
        .unwrap();
    }

    let page1 = db.list_logs("", "", "", 3, 0).unwrap();
    assert_eq!(page1.len(), 3);
    let page2 = db.list_logs("", "", "", 3, 3).unwrap();
    assert_eq!(page2.len(), 2);
}

#[test]
fn clear_logs() {
    let (db, _dir) = make_storage();
    db.insert_log("2026-04-15T10:00:00Z", "engine", "info", "msg1", None, None)
        .unwrap();
    db.insert_log("2026-04-15T10:01:00Z", "engine", "info", "msg2", None, None)
        .unwrap();

    let deleted = db.clear_logs().unwrap();
    assert_eq!(deleted, 2);

    let logs = db.list_logs("", "", "", 10, 0).unwrap();
    assert!(logs.is_empty());
}

#[test]
fn clear_logs_empty() {
    let (db, _dir) = make_storage();
    let deleted = db.clear_logs().unwrap();
    assert_eq!(deleted, 0);
}

#[test]
fn cleanup_old_logs() {
    let (db, _dir) = make_storage();
    db.insert_log(
        "2020-01-01T00:00:00Z",
        "engine",
        "info",
        "old log",
        None,
        None,
    )
    .unwrap();
    db.insert_log(
        "2099-12-31T00:00:00Z",
        "engine",
        "info",
        "future log",
        None,
        None,
    )
    .unwrap();

    let deleted = db.cleanup_old_logs(1).unwrap();
    assert_eq!(deleted, 1);

    let logs = db.list_logs("", "", "", 10, 0).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].summary, "future log");
}

#[test]
fn reset_daily_usage_no_match() {
    let (db, _dir) = make_storage();
    let mut acc = sample_account("a1", "doubao");
    acc.usage_date = "2026-04-15".to_string();
    db.insert_account(&acc).unwrap();

    let reset = db.reset_daily_usage("2026-04-15").unwrap();
    assert_eq!(reset, 0);
}

#[test]
fn delete_nonexistent_account() {
    let (db, _dir) = make_storage();
    db.delete_account("nonexistent").unwrap();
}

#[path = "tests_resilience.rs"]
mod tests_resilience;
