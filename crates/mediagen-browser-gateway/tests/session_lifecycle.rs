use mediagen_browser_gateway::{
    BrowserClipboardAction, BrowserClipboardRequest, BrowserGateway, BrowserGatewayConfig,
    BrowserGatewayError, BrowserInputRequest, BrowserResizeRequest, BrowserSessionCreateRequest,
    BrowserViewport, BrowserWheelInput,
};

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
