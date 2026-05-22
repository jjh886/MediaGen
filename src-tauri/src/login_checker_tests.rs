use super::*;
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

// ── check_all_accounts 集成测试 ──

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
    let (checked, expired) = check_all_accounts(&storage, &plugin_loader).await;
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
    let (checked, expired) = check_all_accounts(&storage, &plugin_loader).await;
    assert_eq!(checked, 0);
    assert_eq!(expired, 0);
}

#[tokio::test]
async fn check_all_returns_zero_on_empty_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let storage = Arc::new(Mutex::new(Storage::new(&db_path).unwrap()));
    let plugin_loader = Arc::new(PluginLoader::empty());

    let (checked, expired) = check_all_accounts(&storage, &plugin_loader).await;
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
    let (checked, expired) = check_all_accounts(&storage, &plugin_loader).await;
    assert_eq!(checked, 0);
    assert_eq!(expired, 0);
}
