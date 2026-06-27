use crate::storage::Storage;
use crate::types::{LoginState, PlatformAccount, TaskKind, TaskRecord, TaskStatus};
use chrono::Utc;
use tempfile::tempdir;

pub(super) fn make_storage() -> (Storage, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.db");
    let storage = Storage::new(&path).unwrap();
    (storage, dir)
}

pub(super) fn sample_account(id: &str, platform: &str) -> PlatformAccount {
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
        task_interval_seconds: 0,
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

pub(super) fn sample_task(id: &str) -> TaskRecord {
    TaskRecord {
        id: id.to_string(),
        platform: "test_platform".to_string(),
        task_kind: TaskKind::Image,
        model_id: "model-v1".to_string(),
        prompt: "一只猫".to_string(),
        account_id: Some("acc-1".to_string()),
        status: TaskStatus::Pending,
        result_url: None,
        storage_key: None,
        error_message: None,
        result_data: None,
        attempts: Vec::new(),
        created_at: Utc::now(),
        completed_at: None,
    }
}
