use super::*;
use crate::login_checker_flow::{
    classify_flow_value, persist_account_outcome, AccountCheckOutcome,
};
use crate::sync_ext::MutexExt;

// ── resolve_field 测试 ──

#[test]
fn resolve_nested_field() {
    let json = serde_json::json!({"user": {"email": "a@b.com"}});
    assert_eq!(resolve_field(&json, "user.email"), Some("a@b.com".into()));
}

#[test]
fn resolve_empty_string_is_none() {
    let json = serde_json::json!({"data": {"user_id_str": ""}});
    assert_eq!(resolve_field(&json, "data.user_id_str"), None);
}

#[test]
fn resolve_missing_field_is_none() {
    let json = serde_json::json!({"data": {}});
    assert_eq!(resolve_field(&json, "data.avatar_url"), None);
}

#[test]
fn resolve_top_level_string() {
    let json = serde_json::json!({"name": "alice"});
    assert_eq!(resolve_field(&json, "name"), Some("alice".into()));
}

#[test]
fn resolve_number_field() {
    let json = serde_json::json!({"data": {"count": 42}});
    assert_eq!(resolve_field(&json, "data.count"), Some("42".into()));
}

#[test]
fn resolve_float_number_field() {
    let json = serde_json::json!({"score": 3.14});
    assert_eq!(resolve_field(&json, "score"), Some("3.14".into()));
}

#[test]
fn resolve_deeply_nested() {
    let json = serde_json::json!({"a": {"b": {"c": {"d": "deep"}}}});
    assert_eq!(resolve_field(&json, "a.b.c.d"), Some("deep".into()));
}

#[test]
fn resolve_null_field_is_none() {
    let json = serde_json::json!({"data": null});
    assert_eq!(resolve_field(&json, "data"), None);
}

#[test]
fn resolve_bool_field_is_none() {
    let json = serde_json::json!({"active": true});
    assert_eq!(resolve_field(&json, "active"), None);
}

#[test]
fn resolve_array_field_is_none() {
    let json = serde_json::json!({"items": [1, 2, 3]});
    assert_eq!(resolve_field(&json, "items"), None);
}

#[test]
fn resolve_object_field_is_none() {
    let json = serde_json::json!({"nested": {"inner": "val"}});
    assert_eq!(resolve_field(&json, "nested"), None);
}

#[test]
fn resolve_partial_path_missing() {
    let json = serde_json::json!({"a": {"b": "val"}});
    assert_eq!(resolve_field(&json, "a.x.y"), None);
}

// ── build_cookie_header 测试 ──

#[test]
fn build_cookie_filters_by_domain() {
    let json = r#"[
        {"name":"a","value":"1","domain":".google.com"},
        {"name":"b","value":"2","domain":".doubao.com"},
        {"name":"c","value":"3","domain":"labs.google"}
    ]"#;
    let header = build_cookie_header(json, "google");
    assert!(header.contains("a=1"));
    assert!(header.contains("c=3"));
    assert!(!header.contains("b=2"));
}

#[test]
fn empty_cookies_returns_empty() {
    assert!(build_cookie_header("[]", "google").is_empty());
    assert!(build_cookie_header("", "google").is_empty());
}

#[test]
fn invalid_json_returns_empty() {
    assert!(build_cookie_header("not json at all", "google").is_empty());
    assert!(build_cookie_header("{}", "google").is_empty());
}

#[test]
fn cookie_without_domain_included() {
    let json = r#"[{"name":"sid","value":"abc123"}]"#;
    let header = build_cookie_header(json, "example");
    assert_eq!(header, "sid=abc123");
}

#[test]
fn multiple_cookies_joined_with_semicolon() {
    let json = r#"[
        {"name":"a","value":"1"},
        {"name":"b","value":"2"},
        {"name":"c","value":"3"}
    ]"#;
    let header = build_cookie_header(json, "any");
    assert_eq!(header.matches("; ").count(), 2);
    assert!(header.contains("a=1"));
    assert!(header.contains("b=2"));
    assert!(header.contains("c=3"));
}

#[test]
fn cookie_domain_with_leading_dot_stripped() {
    let json = r#"[{"name":"sid","value":"x","domain":".doubao.com"}]"#;
    let header = build_cookie_header(json, "doubao");
    assert_eq!(header, "sid=x");
}

#[test]
fn cookie_domain_no_match_returns_empty() {
    let json = r#"[{"name":"a","value":"1","domain":".google.com"}]"#;
    let header = build_cookie_header(json, "baidu");
    assert!(header.is_empty());
}

#[test]
fn cookie_special_characters_in_value() {
    let json = r#"[{"name":"token","value":"abc=def+ghi/jkl"}]"#;
    let header = build_cookie_header(json, "any");
    assert_eq!(header, "token=abc=def+ghi/jkl");
}

// ── CookieEntry serde 测试 ──

#[test]
fn cookie_entry_deserialize_full() {
    let json = r#"{"name":"sid","value":"123","domain":".example.com"}"#;
    let entry: CookieEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.name, "sid");
    assert_eq!(entry.value, "123");
    assert_eq!(entry.domain, Some(".example.com".to_string()));
}

#[test]
fn cookie_entry_deserialize_no_domain() {
    let json = r#"{"name":"sid","value":"123"}"#;
    let entry: CookieEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.name, "sid");
    assert!(entry.domain.is_none());
}

// ── 过期时间检测 ──

#[test]
fn expired_session_detected() {
    let json = serde_json::json!({
        "user": {"email": "a@b.com"},
        "expires": "2020-01-01T00:00:00.000Z"
    });
    assert!(resolve_field(&json, "user.email").is_some());
    let expires_str = resolve_field(&json, "expires").unwrap();
    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_str).unwrap();
    assert!(expires_at < chrono::Utc::now(), "过去的时间应判定为过期");
}

#[test]
fn future_session_valid() {
    let json = serde_json::json!({
        "user": {"email": "a@b.com"},
        "expires": "2099-01-01T00:00:00.000Z"
    });
    let expires_str = resolve_field(&json, "expires").unwrap();
    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_str).unwrap();
    assert!(expires_at > chrono::Utc::now(), "未来的时间应判定为有效");
}

#[test]
fn unauthorized_status_is_login_expired() {
    let reason = login_expired_reason_for_status(StatusCode::UNAUTHORIZED);
    assert!(reason.unwrap().contains("登录凭据无效"));
}

#[test]
fn redirect_status_is_login_expired() {
    let reason = login_expired_reason_for_status(StatusCode::FOUND);
    assert!(reason.unwrap().contains("重定向到登录页"));
}

#[test]
fn forbidden_status_has_no_conclusive_expired_reason() {
    assert!(login_expired_reason_for_status(StatusCode::FORBIDDEN).is_none());
}

#[test]
fn forbidden_status_is_not_inconclusive_by_default() {
    let config = test_login_check(vec![]);
    assert!(!is_inconclusive_status(StatusCode::FORBIDDEN, &config));
}

#[test]
fn forbidden_status_can_be_configured_as_inconclusive() {
    let config = test_login_check(vec![403]);
    assert!(is_inconclusive_status(StatusCode::FORBIDDEN, &config));
}

#[test]
fn classify_flow_available_extracts_account_patch() {
    let value = serde_json::json!({
        "logged_in": true,
        "status": "available",
        "account": {
            "api_token": "access-new",
            "refresh_token": "refresh-new",
            "email": "codex@example.com"
        }
    });

    match classify_flow_value(&value).expect("应该能解释健康检查结果") {
        AccountCheckOutcome::Valid(Some(patch)) => {
            assert_eq!(patch.api_token.as_deref(), Some("access-new"));
            assert_eq!(patch.refresh_token.as_deref(), Some("refresh-new"));
            assert_eq!(patch.email.as_deref(), Some("codex@example.com"));
        }
        _ => panic!("available 结果应该被判定为有效账号"),
    }
}

#[test]
fn classify_flow_inconclusive_does_not_expire_account() {
    let value = serde_json::json!({
        "logged_in": null,
        "status": "inconclusive",
        "reason": "HTTP 403"
    });

    match classify_flow_value(&value).expect("应该能解释不确定结果") {
        AccountCheckOutcome::Inconclusive(reason) => assert_eq!(reason, "HTTP 403"),
        _ => panic!("inconclusive 结果不应该被判定为过期"),
    }
}

#[test]
fn missing_success_field_is_inconclusive_not_expired() {
    let config = test_login_check(vec![]);
    let value = serde_json::json!({"user": {}, "authStatus": "logged_in"});

    match classify_login_json(&value, &config) {
        AccountCheckOutcome::Inconclusive(reason) => {
            assert!(reason.contains("user.email"));
        }
        _ => panic!("字段缺失只能说明 HTTP 探测不确定，不能直接判登录过期"),
    }
}

#[test]
fn future_expiry_can_validate_session_even_without_success_field() {
    let mut config = test_login_check(vec![]);
    config.expires_field = Some("expires".to_string());
    let value = serde_json::json!({"user": {}, "expires": "2099-01-01T00:00:00.000Z"});

    match classify_login_json(&value, &config) {
        AccountCheckOutcome::Valid(None) => {}
        _ => panic!("明确的未来 expires 应判定为有效 session"),
    }
}

#[test]
fn past_expiry_is_still_conclusive_login_expired() {
    let mut config = test_login_check(vec![]);
    config.expires_field = Some("expires".to_string());
    let value = serde_json::json!({"user": {}, "expires": "2020-01-01T00:00:00.000Z"});

    match classify_login_json(&value, &config) {
        AccountCheckOutcome::Expired(reason) => assert!(reason.contains("session 已过期")),
        _ => panic!("明确的过去 expires 应判定为登录过期"),
    }
}

#[test]
fn login_expired_accounts_are_checked_for_recovery() {
    assert!(should_check_login_state(&LoginState::Available));
    assert!(should_check_login_state(&LoginState::LoginExpired));
    assert!(!should_check_login_state(&LoginState::Disabled));
    assert!(!should_check_login_state(&LoginState::Cooldown));
}

// ── check_all_accounts 集成测试 ──

fn test_login_check(inconclusive_statuses: Vec<u16>) -> LoginCheck {
    LoginCheck {
        url: "https://example.test/session".to_string(),
        cookie_domain: "example.test".to_string(),
        success_field: "user.email".to_string(),
        expires_field: None,
        inconclusive_statuses,
    }
}

fn make_test_account(
    id: &str,
    platform: &str,
    state: LoginState,
    cookies: &str,
) -> PlatformAccount {
    PlatformAccount {
        id: id.to_string(),
        platform: platform.to_string(),
        name: format!("test-{id}"),
        notes: String::new(),
        login_state: state,
        login_state_reason: String::new(),
        login_state_updated_at: chrono::Utc::now(),
        cookies: cookies.to_string(),
        api_token: String::new(),
        refresh_token: String::new(),
        token_expires_at: None,
        profile_dir: String::new(),
        tier: String::new(),
        concurrency_limit: 1,
        daily_limit: 100,
        total_limit: 1000,
        used_today: 0,
        used_total: 0,
        usage_date: String::new(),
        cooldown_until: None,
        user_id: String::new(),
        username: String::new(),
        email: String::new(),
        avatar_url: String::new(),
        password_encrypted: String::new(),
        created_at: chrono::Utc::now(),
        last_check_at: None,
        last_refreshed_at: None,
        last_used_at: None,
    }
}

#[test]
fn valid_check_restores_previously_expired_account() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let storage = Arc::new(Mutex::new(Storage::new(&db_path).unwrap()));
    let account = make_test_account(
        "recoverable",
        "chatgpt",
        LoginState::LoginExpired,
        r#"[{"name":"sid","value":"123","domain":".chatgpt.com"}]"#,
    );

    {
        let s = storage.lock_recover();
        s.insert_account(&account).unwrap();
    }

    let expired = persist_account_outcome(&storage, &account, AccountCheckOutcome::Valid(None));
    let restored = storage
        .lock_recover()
        .get_account("recoverable")
        .unwrap()
        .unwrap();

    assert!(!expired);
    assert_eq!(restored.login_state, LoginState::Available);
    assert_eq!(restored.login_state_reason, "账号健康检查通过");
    assert!(restored.last_check_at.is_some());
}

#[tokio::test]
async fn check_all_skips_non_available_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let storage = Arc::new(Mutex::new(Storage::new(&db_path).unwrap()));

    {
        let s = storage.lock_recover();
        s.insert_account(&make_test_account(
            "a1",
            "flow",
            LoginState::LoginExpired,
            "[]",
        ))
        .unwrap();
        s.insert_account(&make_test_account("a2", "flow", LoginState::Disabled, "[]"))
            .unwrap();
        s.insert_account(&make_test_account("a3", "flow", LoginState::Cooldown, "[]"))
            .unwrap();
    }

    let plugin_loader = Arc::new(PluginLoader::empty());
    let (checked, expired) = check_all_accounts(&storage, &plugin_loader, None).await;
    assert_eq!(checked, 0);
    assert_eq!(expired, 0);
}

#[tokio::test]
async fn check_all_skips_platform_without_login_check() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let storage = Arc::new(Mutex::new(Storage::new(&db_path).unwrap()));

    {
        let s = storage.lock_recover();
        s.insert_account(&make_test_account(
            "a1",
            "unknown_platform",
            LoginState::Available,
            r#"[{"name":"sid","value":"123","domain":".example.com"}]"#,
        ))
        .unwrap();
    }

    let plugin_loader = Arc::new(PluginLoader::empty());
    let (checked, expired) = check_all_accounts(&storage, &plugin_loader, None).await;
    assert_eq!(checked, 0);
    assert_eq!(expired, 0);
}

#[tokio::test]
async fn check_all_returns_zero_on_empty_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let storage = Arc::new(Mutex::new(Storage::new(&db_path).unwrap()));
    let plugin_loader = Arc::new(PluginLoader::empty());

    let (checked, expired) = check_all_accounts(&storage, &plugin_loader, None).await;
    assert_eq!(checked, 0);
    assert_eq!(expired, 0);
}

#[tokio::test]
async fn check_all_mixed_states_only_checks_available() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let storage = Arc::new(Mutex::new(Storage::new(&db_path).unwrap()));

    {
        let s = storage.lock_recover();
        s.insert_account(&make_test_account(
            "a1",
            "flow",
            LoginState::Available,
            "[]",
        ))
        .unwrap();
        s.insert_account(&make_test_account(
            "a2",
            "flow",
            LoginState::LoginExpired,
            "[]",
        ))
        .unwrap();
        s.insert_account(&make_test_account(
            "a3",
            "flow",
            LoginState::DailyQuotaExhausted,
            "[]",
        ))
        .unwrap();
        s.insert_account(&make_test_account(
            "a4",
            "flow",
            LoginState::TotalQuotaExhausted,
            "[]",
        ))
        .unwrap();
        s.insert_account(&make_test_account(
            "a5",
            "flow",
            LoginState::VerificationRequired,
            "[]",
        ))
        .unwrap();
    }

    let plugin_loader = Arc::new(PluginLoader::empty());
    let (checked, expired) = check_all_accounts(&storage, &plugin_loader, None).await;
    assert_eq!(checked, 0);
    assert_eq!(expired, 0);
}
