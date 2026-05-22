use super::*;

#[test]
fn merge_userinfo_from_json() {
    let mut result = LoginResult::default();
    let info = serde_json::json!({
        "userId": "123",
        "username": "test",
        "email": "t@t.com",
        "avatarUrl": "https://img/a.png",
        "tier": "pro",
        "credits": 50
    });
    merge_userinfo(&mut result, &info);
    assert_eq!(result.user_id, "123");
    assert_eq!(result.username, "test");
    assert_eq!(result.email, "t@t.com");
    assert_eq!(result.avatar_url, "https://img/a.png");
    assert_eq!(result.tier, "pro");
    assert_eq!(result.credits, 50);
}

#[test]
fn merge_skips_empty_values() {
    let mut result = LoginResult {
        username: "original".to_string(),
        ..Default::default()
    };
    let info = serde_json::json!({ "username": "", "email": "new@t.com" });
    merge_userinfo(&mut result, &info);
    assert_eq!(result.username, "original"); // 空值不覆盖
    assert_eq!(result.email, "new@t.com");
}

/// 回归：复用已有 profile 时 prepare_profile 必须先 kill 残留 Chrome，
/// 再清理锁文件/修 prefs，否则 Windows 不开窗、macOS 弹 profile-error 对话框。
#[test]
fn prepare_profile_cleans_existing_profile_idempotently() {
    let tmp = std::env::temp_dir().join(format!(
        "frame64-prepare-profile-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    // 模拟残留的 Singleton 文件
    for lock in &["SingletonLock", "SingletonSocket", "SingletonCookie"] {
        std::fs::write(tmp.join(lock), b"stale").unwrap();
    }

    let config = BrowserLoginConfig {
        chrome_path: None,
        profile_dir: tmp.clone(),
        login_url: String::new(),
        detection: LoginDetection::CdpCookies {
            cookie_names: vec![],
        },
        userinfo_script: None,
        userinfo_await_promise: false,
    };

    // 不应 panic；kill 对无匹配进程是 no-op
    prepare_profile(&config).unwrap();

    // Singleton 锁应被清掉
    for lock in &["SingletonLock", "SingletonSocket", "SingletonCookie"] {
        assert!(!tmp.join(lock).exists(), "{} 未被清理", lock);
    }

    std::fs::remove_dir_all(&tmp).ok();
}
