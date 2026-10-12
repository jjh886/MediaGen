use std::collections::HashMap;

use mediagen_browser_gateway::{
    BrowserGateway, BrowserGatewayConfig, BrowserResizeRequest, BrowserSessionCreateRequest,
};
use serde_json::json;

#[tokio::test]
#[ignore = "需要本机 CloakBrowser"]
async fn resizing_keeps_screencast_frames_arriving() {
    use std::time::Duration;
    let root = std::env::temp_dir().join(format!("mediagen-resize-{}", uuid::Uuid::new_v4()));
    let gateway = BrowserGateway::new(BrowserGatewayConfig {
        profile_root: root.clone(),
        ..BrowserGatewayConfig::default()
    });
    let session = gateway
        .create_session(BrowserSessionCreateRequest {
            start_url: "data:text/html,<body style='background:coral'>Resize probe</body>".into(),
            metadata: HashMap::from([("proxy_mode".into(), json!("direct"))]),
            ..Default::default()
        })
        .await
        .unwrap()
        .session;
    // 初始导航异步执行；先确认测试页面已加载，再测试视口调整。
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if gateway
                .evaluate_script(
                    &session.id,
                    "document.body?.textContent.includes('Resize probe') === true",
                    false,
                )
                .await
                .unwrap()
                == json!(true)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("测试页面必须完成初始导航");
    for width in [800, 1100, 900] {
        let after = gateway
            .events_since(&session.id, 0)
            .unwrap()
            .last()
            .map_or(0, |e| e.sequence);
        tokio::time::timeout(
            Duration::from_secs(5),
            gateway.resize_session(
                &session.id,
                BrowserResizeRequest {
                    width,
                    height: 700,
                    device_scale_factor: Some(1.0),
                },
            ),
        )
        .await
        .expect("调整视口不能等待重启投屏")
        .unwrap();
        gateway
            .evaluate_script(
                &session.id,
                &format!(
                    "document.body.style.background='rgb({},100,150)'",
                    width % 255
                ),
                false,
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let events = gateway.events_since(&session.id, after).unwrap();
                if events.iter().any(|event| {
                    event.event == "frame"
                        && event.data["viewport"]["width"] == width
                        && event.data["image"]
                            .as_str()
                            .is_some_and(|value| !value.is_empty())
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|error| {
            let frame_sizes: Vec<_> = gateway
                .events_since(&session.id, after)
                .unwrap()
                .iter()
                .filter(|event| event.event == "frame")
                .map(|event| event.data["viewport"].clone())
                .collect();
            panic!("调整视口 {width} 后必须继续收到真实画面: {error}; 帧视口: {frame_sizes:?}");
        });
    }
    gateway.complete_session(&session.id).await.unwrap();
    let _ = std::fs::remove_dir_all(root);
}
