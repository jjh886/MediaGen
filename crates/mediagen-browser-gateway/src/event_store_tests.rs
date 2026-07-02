use serde_json::json;

use crate::{BrowserGateway, BrowserGatewayConfig, BrowserSessionCreateRequest};

#[tokio::test]
async fn frame_events_keep_only_latest_image() {
    let gateway = BrowserGateway::new(BrowserGatewayConfig {
        launch_browser: false,
        ..BrowserGatewayConfig::default()
    });
    let session = gateway
        .create_session(BrowserSessionCreateRequest::default())
        .await
        .unwrap()
        .session;

    gateway
        .push_event(&session.id, "frame", json!({ "image": "old" }))
        .unwrap();
    gateway
        .push_event(&session.id, "frame", json!({ "image": "new" }))
        .unwrap();

    let frames: Vec<_> = gateway
        .events_since(&session.id, 0)
        .unwrap()
        .into_iter()
        .filter(|event| event.event == "frame")
        .collect();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].data["image"], "new");
}
