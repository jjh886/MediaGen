use super::*;
use crate::types::LoginState;
use chrono::Utc;
use tempfile::tempdir;

fn make_manager() -> (AccountManager, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.db");
    let storage = Storage::new(&path).unwrap();
    let mgr = AccountManager::new(Arc::new(Mutex::new(storage)));
    (mgr, dir)
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
        api_token: String::new(),
        refresh_token: String::new(),
        token_expires_at: None,
        profile_dir: String::new(),
        tier: "free".to_string(),
        concurrency_limit: 1,
        daily_limit: 0,
        total_limit: 0,
        used_today: 0,
        used_total: 0,
        usage_date: "2026-04-13".to_string(),
        cooldown_until: None,
        user_id: String::new(),
        username: String::new(),
        email: String::new(),
        avatar_url: String::new(),
        password_encrypted: String::new(),
        created_at: now,
        last_check_at: None,
        last_refreshed_at: None,
        last_used_at: None,
    }
}

#[tokio::test]
async fn test_add_and_list() {
    let (mgr, _dir) = make_manager();
    mgr.add_account(sample_account("a1", "doubao"))
        .await
        .unwrap();
    mgr.add_account(sample_account("a2", "doubao"))
        .await
        .unwrap();
    mgr.add_account(sample_account("a3", "kling"))
        .await
        .unwrap();

    let list = mgr.list_accounts("doubao").await.unwrap();
    assert_eq!(list.len(), 2);

    let acc = mgr.get_account("a1").await.unwrap();
    assert!(acc.is_some());
    assert_eq!(acc.unwrap().id, "a1");
}

#[tokio::test]
async fn test_increment_usage_daily_limit() {
    let (mgr, _dir) = make_manager();
    let mut acc = sample_account("a1", "doubao");
    acc.daily_limit = 3;
    mgr.add_account(acc).await.unwrap();

    for _ in 0..3 {
        mgr.increment_usage("a1").await.unwrap();
    }

    let acc = mgr.get_account("a1").await.unwrap().unwrap();
    assert_eq!(acc.used_today, 3);
    assert_eq!(acc.login_state, LoginState::DailyQuotaExhausted);
}

#[tokio::test]
async fn test_increment_usage_total_limit() {
    let (mgr, _dir) = make_manager();
    let mut acc = sample_account("a1", "doubao");
    acc.total_limit = 2;
    mgr.add_account(acc).await.unwrap();

    for _ in 0..2 {
        mgr.increment_usage("a1").await.unwrap();
    }

    let acc = mgr.get_account("a1").await.unwrap().unwrap();
    assert_eq!(acc.used_total, 2);
    assert_eq!(acc.login_state, LoginState::TotalQuotaExhausted);
}

#[tokio::test]
async fn test_reset_daily_quotas() {
    let (mgr, _dir) = make_manager();
    let mut acc = sample_account("a1", "doubao");
    acc.daily_limit = 1;
    acc.login_state = LoginState::DailyQuotaExhausted;
    acc.used_today = 1;
    acc.usage_date = "2026-04-12".to_string(); // 昨天的日期，确保 reset 能命中
    mgr.add_account(acc).await.unwrap();

    let count = mgr.reset_daily_quotas().await.unwrap();
    assert_eq!(count, 1);

    let acc = mgr.get_account("a1").await.unwrap().unwrap();
    assert_eq!(acc.used_today, 0);
    assert_eq!(acc.login_state, LoginState::Available);
}

#[test]
fn test_classify_browser_selector_error_no_state_change() {
    assert_eq!(
        classify_failure("浏览器错误: 未找到文本为 '创建图片' 的元素"),
        None
    );
    assert_eq!(
        classify_failure("平台错误: 未进入图片模式，且未找到创建图片入口"),
        None
    );
}

#[test]
fn test_classify_chatgpt_image_mode_failure_short_cooldown() {
    assert_eq!(
        classify_chatgpt_flow_failure("chatgpt", "平台错误: 未进入图片模式，且未找到创建图片入口"),
        Some((LoginState::Cooldown, 180))
    );
    assert_eq!(
        classify_chatgpt_flow_failure("doubao", "平台错误: 未进入图片模式，且未找到创建图片入口"),
        None
    );
}

#[test]
fn test_classify_quota_exhausted_disables_account() {
    assert_eq!(
        classify_failure("平台错误: 账号额度耗尽 quota_exhausted"),
        Some((LoginState::Disabled, 0))
    );
}

#[test]
fn test_classify_rate_limit_no_state_change() {
    assert_eq!(classify_failure("platform error: rate limited"), None);
}

#[test]
fn test_extract_rate_limit_cooldown_hint_from_openai_message() {
    let msg = "You're generating images too quickly. To ensure the best experience for everyone, we have rate limits in place. Please wait for 29 minutes before generating more images.";
    assert_eq!(extract_rate_limit_cooldown_hint(msg), Some(29 * 60));
}

#[test]
fn test_extract_rate_limit_cooldown_hint_fallbacks_when_no_duration() {
    assert_eq!(
        extract_rate_limit_cooldown_hint("platform error: rate_limit_exceeded"),
        Some(-1)
    );
}

#[test]
fn test_extract_rate_limit_cooldown_hint_from_chinese_modal() {
    let msg = "平台错误: 生成过于频繁: 请求过于频繁 你的请求过于频繁。为保障数据安全，我们已暂时限制你访问对话记录。 请稍等几分钟后再重试。";
    assert_eq!(extract_rate_limit_cooldown_hint(msg), Some(180));
}

#[test]
fn test_extract_rate_limit_cooldown_hint_from_chinese_minutes() {
    let msg = "平台错误: 生成过于频繁: 请求过于频繁 请稍等8分钟后再重试。";
    assert_eq!(extract_rate_limit_cooldown_hint(msg), Some(8 * 60));
}

#[test]
fn test_truncate_reason_handles_multibyte_chars() {
    let reason = truncate_reason("平台错误: 账号额度耗尽: 画面风格描述", 10);
    assert_eq!(reason.chars().count(), 10);
}
