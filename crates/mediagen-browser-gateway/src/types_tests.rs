use serde_json::json;

use crate::input_types::{
    BrowserClipboardAction, BrowserClipboardRequest, BrowserInputRequest, BrowserKeyInput,
};
use crate::types::*;

#[test]
fn create_request_resolves_defaults_and_alias_urls() {
    let config = BrowserGatewayConfig {
        default_width: 1200,
        default_height: 700,
        ..BrowserGatewayConfig::default()
    };
    let empty = BrowserSessionCreateRequest::default();
    assert_eq!(empty.resolved_url(), DEFAULT_URL);
    assert_eq!(empty.resolved_platform(), "browser");
    assert_eq!(empty.resolved_runtime_id(), DEFAULT_RUNTIME_ID);
    assert_eq!(
        empty.resolved_viewport(&config),
        BrowserViewport {
            width: 1200,
            height: 700
        }
    );

    let request = BrowserSessionCreateRequest {
        platform: " platform-a ".to_string(),
        start_url: "https://start.test".to_string(),
        url: Some("https://url.test".to_string()),
        initial_url: Some(" https://initial.test ".to_string()),
        runtime_id: Some(" cloakbrowser ".to_string()),
        width: Some(10),
        height: Some(9000),
        device_scale_factor: Some(2.5),
        ..BrowserSessionCreateRequest::default()
    };

    assert_eq!(request.resolved_url(), "https://initial.test");
    assert_eq!(request.resolved_platform(), "platform-a");
    assert_eq!(request.resolved_runtime_id(), "cloakbrowser");
    assert_eq!(request.resolved_device_scale_factor(), 2.0);
    assert_eq!(
        request.resolved_viewport(&config),
        BrowserViewport {
            width: 480,
            height: 1800
        }
    );
}

#[test]
fn explicit_viewport_overrides_width_height_and_clamps() {
    let request = BrowserSessionCreateRequest {
        width: Some(1000),
        height: Some(1000),
        viewport: Some(BrowserViewport {
            width: 3000,
            height: 100,
        }),
        ..BrowserSessionCreateRequest::default()
    };

    assert_eq!(
        request.resolved_viewport(&BrowserGatewayConfig::default()),
        BrowserViewport {
            width: 2560,
            height: 360
        }
    );
}

#[test]
fn input_request_accepts_frontend_alias_fields() {
    let mouse: BrowserInputRequest = serde_json::from_value(json!({
        "kind": "mouse",
        "event": "down",
        "x": 12,
        "y": 24,
        "button": "left",
        "clickCount": 2,
        "readCursor": false
    }))
    .unwrap();
    let BrowserInputRequest::Mouse(mouse) = mouse else {
        panic!("expected mouse input");
    };
    assert_eq!(mouse.click_count, 2);
    assert!(!mouse.read_cursor);

    let wheel: BrowserInputRequest = serde_json::from_value(json!({
        "kind": "wheel",
        "deltaX": 1,
        "delta_y": 2,
        "read_cursor": false
    }))
    .unwrap();
    let BrowserInputRequest::Wheel(wheel) = wheel else {
        panic!("expected wheel input");
    };
    assert_eq!(wheel.delta_x, 1.0);
    assert_eq!(wheel.delta_y, 2.0);
    assert!(!wheel.read_cursor);
}

#[test]
fn key_clipboard_and_screenshot_defaults_match_ui_contract() {
    let key: BrowserKeyInput = serde_json::from_value(json!({
        "event": "down",
        "key": "v",
        "ctrlKey": true,
        "meta_key": false
    }))
    .unwrap();
    assert!(key.ctrl_key);
    assert!(!key.meta_key);

    let clipboard: BrowserClipboardRequest = serde_json::from_value(json!({})).unwrap();
    assert!(matches!(clipboard.action, BrowserClipboardAction::Write));

    let screenshot: BrowserScreenshotRequest = serde_json::from_value(json!({})).unwrap();
    assert_eq!(screenshot.format, "jpeg");
    assert!(!screenshot.full_page);
}
