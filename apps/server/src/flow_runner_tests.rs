use std::sync::Arc;

use mediagen_browser_gateway::{
    BrowserGateway, BrowserGatewayConfig, BrowserSessionCreateRequest, BrowserSessionPurpose,
    BrowserSessionStatus,
};
use serde_json::json;

use crate::flow_browser_lifecycle::FlowBrowserLifecycle;

use super::screenshot_request_from_params;

fn test_gateway() -> Arc<BrowserGateway> {
    Arc::new(BrowserGateway::new(BrowserGatewayConfig {
        launch_browser: false,
        ..BrowserGatewayConfig::default()
    }))
}

async fn task_session(
    gateway: &BrowserGateway,
    account_id: &str,
    task_id: &str,
    profile_dir: &str,
) -> mediagen_browser_gateway::BrowserSession {
    gateway
        .create_session(BrowserSessionCreateRequest {
            purpose: BrowserSessionPurpose::TaskTakeover,
            account_id: Some(account_id.to_string()),
            task_id: Some(task_id.to_string()),
            profile_dir: Some(profile_dir.to_string()),
            ..BrowserSessionCreateRequest::default()
        })
        .await
        .unwrap()
        .session
}

#[test]
fn screenshot_request_accepts_flow_params_and_defaults() {
    let default = screenshot_request_from_params(&json!({ "session_id": "browser-1" }))
        .expect("default screenshot params");
    assert_eq!(default.format, "jpeg");
    assert_eq!(default.quality, None);
    assert!(!default.full_page);

    let custom = screenshot_request_from_params(&json!({
        "session_id": "browser-1",
        "format": "png",
        "quality": 80,
        "full_page": true
    }))
    .expect("custom screenshot params");
    assert_eq!(custom.format, "png");
    assert_eq!(custom.quality, Some(80));
    assert!(custom.full_page);
}

#[tokio::test]
async fn failed_task_only_cancels_its_own_account_browser() {
    let gateway = test_gateway();
    let first = task_session(&gateway, "account-1", "task-1", "/tmp/profile-1").await;
    let second = task_session(&gateway, "account-2", "task-2", "/tmp/profile-2").await;
    let mut lifecycle = FlowBrowserLifecycle::new(gateway.clone(), true);
    lifecycle.remember_open("browser.open", &json!({ "session_id": first.id }));

    lifecycle.finish(false).await;

    assert_eq!(
        gateway.get_session(&first.id).unwrap().status,
        BrowserSessionStatus::Cancelled
    );
    assert_eq!(
        gateway.get_session(&second.id).unwrap().status,
        BrowserSessionStatus::Active
    );
    assert_eq!(
        gateway.get_session(&second.id).unwrap().task_id.as_deref(),
        Some("task-2")
    );
}

#[tokio::test]
async fn successful_persistent_task_releases_only_its_own_browser() {
    let gateway = test_gateway();
    let first = task_session(&gateway, "account-1", "task-1", "/tmp/profile-1").await;
    let second = task_session(&gateway, "account-2", "task-2", "/tmp/profile-2").await;
    let mut lifecycle = FlowBrowserLifecycle::new(gateway.clone(), true);
    lifecycle.remember_open("browser.open", &json!({ "session_id": first.id }));

    lifecycle.finish(true).await;

    let released = gateway.get_session(&first.id).unwrap();
    assert_eq!(released.status, BrowserSessionStatus::Active);
    assert!(released.task_id.is_none());
    assert_eq!(
        gateway.get_session(&second.id).unwrap().task_id.as_deref(),
        Some("task-2")
    );
}
