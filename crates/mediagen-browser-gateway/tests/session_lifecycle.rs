use std::collections::HashMap;

use mediagen_browser_gateway::{
    BrowserClipboardAction, BrowserClipboardRequest, BrowserGateway, BrowserGatewayConfig,
    BrowserGatewayError, BrowserInputRequest, BrowserResizeRequest, BrowserSessionCreateRequest,
    BrowserSessionPurpose, BrowserViewport, BrowserWheelInput,
};
use serde_json::json;

fn test_gateway() -> BrowserGateway {
    BrowserGateway::new(BrowserGatewayConfig {
        launch_browser: false,
        ..BrowserGatewayConfig::default()
    })
}

#[tokio::test]
async fn session_lifecycle_matches_demo_shape() {
    let gateway = test_gateway();
    let created = gateway
        .create_session(BrowserSessionCreateRequest {
            url: Some("https://example.com/login".to_string()),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .unwrap()
        .session;

    assert_eq!(created.url, "https://example.com/login");
    assert_eq!(created.viewport, BrowserViewport::default());

    let result = gateway
        .resize_session(
            &created.id,
            BrowserResizeRequest {
                width: 9999,
                height: 100,
                device_scale_factor: None,
            },
        )
        .await;
    assert!(matches!(result, Err(BrowserGatewayError::Cdp(_))));

    let input = BrowserInputRequest::Wheel(BrowserWheelInput {
        x: 10.0,
        y: 20.0,
        delta_x: 0.0,
        delta_y: 120.0,
        modifiers: 0,
        read_cursor: true,
    });
    assert!(matches!(
        gateway.dispatch_input(&created.id, input).await,
        Err(BrowserGatewayError::Cdp(_))
    ));

    let written = gateway
        .clipboard(
            &created.id,
            BrowserClipboardRequest {
                action: BrowserClipboardAction::Write,
                text: Some("hello".to_string()),
            },
        )
        .await;
    assert!(matches!(written, Err(BrowserGatewayError::Cdp(_))));

    let events = gateway.events_since(&created.id, 0).unwrap();
    assert!(events.iter().any(|event| event.event == "status"));
}

#[tokio::test]
async fn cancelled_session_rejects_later_actions() {
    let gateway = test_gateway();
    let created = gateway
        .create_session(BrowserSessionCreateRequest::default())
        .await
        .unwrap()
        .session;
    gateway.cancel_session(&created.id).await.unwrap();

    let result = gateway
        .resize_session(
            &created.id,
            BrowserResizeRequest {
                width: 800,
                height: 600,
                device_scale_factor: None,
            },
        )
        .await;
    assert!(matches!(result, Err(BrowserGatewayError::SessionClosed(_))));
}

#[tokio::test]
async fn default_proxy_is_used_only_without_session_override() {
    let gateway = BrowserGateway::new(BrowserGatewayConfig {
        launch_browser: false,
        default_proxy_server: Some("socks5h://127.0.0.1:7890".to_string()),
        ..BrowserGatewayConfig::default()
    });
    let inherited = gateway
        .create_session(BrowserSessionCreateRequest::default())
        .await
        .unwrap()
        .session;
    assert_eq!(
        inherited.metadata["proxy_server"],
        "socks5h://127.0.0.1:7890"
    );

    let mut metadata = HashMap::new();
    metadata.insert("proxy_mode".to_string(), json!("direct"));
    let direct = gateway
        .create_session(BrowserSessionCreateRequest {
            metadata,
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .unwrap()
        .session;
    assert!(direct.metadata.get("proxy_server").is_none());
}

/// 真实 CloakBrowser 冒烟测试：同一 profile 的多个任务必须共享进程并各用独立 Tab。
///
/// 默认忽略，避免普通 CI 依赖本机浏览器；发布安装包前通过 CLOAK_BROWSER_PATH 显式运行。
#[tokio::test]
#[ignore = "需要本机 CloakBrowser"]
async fn same_profile_tasks_use_independent_tabs_in_one_browser() {
    let root = std::env::temp_dir().join(format!("mediagen-profile-tabs-{}", uuid::Uuid::new_v4()));
    let profile_dir = root.join("shared");
    let gateway = BrowserGateway::new(BrowserGatewayConfig {
        profile_root: root.clone(),
        ..BrowserGatewayConfig::default()
    });
    let first = gateway
        .create_session(BrowserSessionCreateRequest {
            purpose: BrowserSessionPurpose::TaskTakeover,
            task_id: Some("task-1".to_string()),
            profile_dir: Some(profile_dir.to_string_lossy().to_string()),
            start_url: "about:blank".to_string(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .expect("第一个任务应启动浏览器")
        .session;
    let second = gateway
        .create_profile_tab_session(BrowserSessionCreateRequest {
            purpose: BrowserSessionPurpose::TaskTakeover,
            task_id: Some("task-2".to_string()),
            profile_dir: Some(profile_dir.to_string_lossy().to_string()),
            start_url: "about:blank".to_string(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .expect("第二个任务应创建 Tab")
        .expect("已有 profile 浏览器时必须复用")
        .session;

    assert_ne!(first.id, second.id);
    assert_eq!(
        gateway
            .evaluate_script(&first.id, "1 + 1", false)
            .await
            .unwrap(),
        json!(2)
    );
    assert_eq!(
        gateway
            .evaluate_script(&second.id, "2 + 2", false)
            .await
            .unwrap(),
        json!(4)
    );

    // 关闭第一个 Tab 后，第二个任务仍应正常执行，不能把整个浏览器进程一起杀掉。
    gateway.complete_session(&first.id).await.unwrap();
    assert_eq!(
        gateway
            .evaluate_script(&second.id, "3 + 3", false)
            .await
            .unwrap(),
        json!(6)
    );
    gateway.complete_session(&second.id).await.unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[ignore = "需要 CloakBrowser 和 MEDIAGEN_BROWSER_PROBE_ORIGIN"]
async fn separate_accounts_share_cookies_without_sharing_private_files() {
    let origin = std::env::var("MEDIAGEN_BROWSER_PROBE_ORIGIN")
        .expect("请设置 MEDIAGEN_BROWSER_PROBE_ORIGIN 为可访问的测试站点");
    let browser_version =
        std::env::var("MEDIAGEN_BROWSER_PROBE_VERSION").unwrap_or_else(|_| "154".to_string());
    let root =
        std::env::temp_dir().join(format!("mediagen-shared-browser-{}", uuid::Uuid::new_v4()));
    let browser_dir = root.join("browser").to_string_lossy().to_string();
    let first_private = root.join("account-a").to_string_lossy().to_string();
    let second_private = root.join("account-b").to_string_lossy().to_string();
    let gateway = BrowserGateway::new(BrowserGatewayConfig {
        profile_root: root.clone(),
        ..BrowserGatewayConfig::default()
    });
    let metadata = HashMap::from([
        ("proxy_mode".to_string(), json!("direct")),
        ("browser_os".to_string(), json!("windows")),
        ("browser_version".to_string(), json!(browser_version)),
    ]);
    let first = gateway
        .create_session(BrowserSessionCreateRequest {
            profile_dir: Some(first_private.clone()),
            browser_profile_dir: Some(browser_dir.clone()),
            start_url: origin.clone(),
            metadata: metadata.clone(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .unwrap()
        .session;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(
        gateway
            .evaluate_script(&first.id, "navigator.platform", false)
            .await
            .unwrap(),
        json!("Win32")
    );
    assert_eq!(
        gateway
            .evaluate_script(&first.id, "navigator.userAgentData.platform", false)
            .await
            .unwrap(),
        json!("Windows")
    );
    let ua = gateway
        .evaluate_script(&first.id, "navigator.userAgent", false)
        .await
        .unwrap();
    assert!(ua
        .as_str()
        .unwrap()
        .contains(&format!("Chrome/{browser_version}.0.0.0")));
    let brands = gateway
        .evaluate_script(&first.id, "navigator.userAgentData.brands", false)
        .await
        .unwrap();
    assert!(brands
        .as_array()
        .unwrap()
        .iter()
        .any(|brand| { brand["brand"] == "Google Chrome" && brand["version"] == browser_version }));
    gateway
        .evaluate_script(
            &first.id,
            "localStorage.setItem('mediagen_probe', 'shared-ok'); document.cookie = 'mediagen_probe=shared-ok; Path=/; SameSite=Lax'; true",
            false,
        )
        .await
        .unwrap();

    let second = gateway
        .create_profile_tab_session(BrowserSessionCreateRequest {
            profile_dir: Some(second_private.clone()),
            browser_profile_dir: Some(browser_dir.clone()),
            start_url: origin,
            metadata,
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .unwrap()
        .expect("相同浏览器资料应复用进程并新开标签页")
        .session;
    assert_ne!(first.profile_dir, second.profile_dir);
    assert_eq!(first.browser_profile_dir, second.browser_profile_dir);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(
        gateway
            .evaluate_script(&second.id, "localStorage.getItem('mediagen_probe')", false)
            .await
            .unwrap(),
        json!("shared-ok")
    );
    let cookie = gateway
        .evaluate_script(&second.id, "document.cookie", false)
        .await
        .unwrap();
    assert!(cookie
        .as_str()
        .unwrap()
        .contains("mediagen_probe=shared-ok"));
    gateway.complete_session(&first.id).await.unwrap();
    assert_eq!(
        gateway
            .evaluate_script(&second.id, "1 + 1", false)
            .await
            .unwrap(),
        json!(2)
    );
    gateway.complete_session(&second.id).await.unwrap();
    let _ = std::fs::remove_dir_all(root);
}

/// 模拟服务重启：新的 Gateway 没有旧内存状态，也必须通过 profile 端口登记接管浏览器。
#[tokio::test]
#[ignore = "需要本机 CloakBrowser"]
async fn restarted_gateway_reuses_registered_profile_browser() {
    let root =
        std::env::temp_dir().join(format!("mediagen-restart-reuse-{}", uuid::Uuid::new_v4()));
    let profile_dir = root.join("shared");
    let config = BrowserGatewayConfig {
        profile_root: root.clone(),
        ..BrowserGatewayConfig::default()
    };
    let first_gateway = BrowserGateway::new(config.clone());
    let first = first_gateway
        .create_session(BrowserSessionCreateRequest {
            profile_dir: Some(profile_dir.to_string_lossy().to_string()),
            start_url: "about:blank".to_string(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .expect("原服务应启动浏览器")
        .session;

    let restarted_gateway = BrowserGateway::new(config);
    let reused = restarted_gateway
        .create_session(BrowserSessionCreateRequest {
            profile_dir: Some(profile_dir.to_string_lossy().to_string()),
            start_url: "about:blank".to_string(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .expect("重启后的服务应接管已有浏览器")
        .session;

    assert_eq!(
        restarted_gateway
            .evaluate_script(&reused.id, "21 * 2", false)
            .await
            .unwrap(),
        json!(42)
    );
    restarted_gateway
        .complete_session(&reused.id)
        .await
        .unwrap();
    first_gateway.complete_session(&first.id).await.unwrap();
    let _ = std::fs::remove_dir_all(root);
}

/// 正常完成登录或任务时必须让 Chromium 刷新 profile，不能直接 kill 丢失刚写入的 Cookie。
#[tokio::test]
#[ignore = "需要本机 CloakBrowser 和网络"]
async fn completed_session_persists_profile_cookie_after_browser_restart() {
    let root =
        std::env::temp_dir().join(format!("mediagen-profile-flush-{}", uuid::Uuid::new_v4()));
    let profile_dir = root.join("shared");
    let gateway = BrowserGateway::new(BrowserGatewayConfig {
        profile_root: root.clone(),
        ..BrowserGatewayConfig::default()
    });
    let first = gateway
        .create_session(BrowserSessionCreateRequest {
            profile_dir: Some(profile_dir.to_string_lossy().to_string()),
            start_url: "https://example.com/".to_string(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .expect("应启动首次浏览器")
        .session;
    let written = gateway
        .evaluate_script(
            &first.id,
            "document.cookie = 'mediagen_flush=present; path=/; max-age=3600; SameSite=Lax'; document.cookie",
            false,
        )
        .await
        .expect("应写入测试 Cookie");
    assert!(written
        .as_str()
        .unwrap_or_default()
        .contains("mediagen_flush=present"));
    gateway.complete_session(&first.id).await.unwrap();

    let reopened = gateway
        .create_session(BrowserSessionCreateRequest {
            profile_dir: Some(profile_dir.to_string_lossy().to_string()),
            start_url: "https://example.com/".to_string(),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .expect("应重新打开同一 profile")
        .session;
    let persisted = gateway
        .evaluate_script(&reopened.id, "document.cookie", false)
        .await
        .expect("应读取重启后的 Cookie");

    assert!(persisted
        .as_str()
        .unwrap_or_default()
        .contains("mediagen_flush=present"));
    gateway.complete_session(&reopened.id).await.unwrap();
    let _ = std::fs::remove_dir_all(root);
}
