use std::collections::HashMap;

use base64::prelude::{Engine as _, BASE64_STANDARD};
use serde_json::json;

use super::{apply_body, proxy_url, ServerPluginHost};

#[test]
fn http_request_defaults_to_account_proxy() {
    assert_eq!(
        proxy_url(&json!({}), "127.0.0.1:1080"),
        Some("socks5://127.0.0.1:1080".to_string())
    );
}

#[test]
fn http_request_can_opt_out_of_account_proxy() {
    assert_eq!(
        proxy_url(&json!({ "proxy": "direct" }), "socks5://127.0.0.1:1080"),
        None
    );
    assert_eq!(
        proxy_url(
            &json!({ "use_account_proxy": false }),
            "socks5://127.0.0.1:1080"
        ),
        None
    );
}

#[test]
fn account_fingerprint_fills_default_http_headers() {
    let mut metadata = HashMap::new();
    metadata.insert("browser_os".to_string(), json!("windows"));
    metadata.insert("browser_version".to_string(), json!("126"));
    let host = ServerPluginHost::new_with_metadata(String::new(), &metadata);

    let headers = host.request_headers(&json!({})).unwrap();

    assert!(headers
        .get("user-agent")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("Chrome/126.0.0.0"));
    assert_eq!(
        headers.get("sec-ch-ua-platform").unwrap().to_str().unwrap(),
        "\"Windows\""
    );
}

#[test]
fn explicit_plugin_user_agent_is_not_overwritten() {
    let mut metadata = HashMap::new();
    metadata.insert("browser_os".to_string(), json!("windows"));
    metadata.insert("browser_version".to_string(), json!("126"));
    let host = ServerPluginHost::new_with_metadata(String::new(), &metadata);

    let headers = host
        .request_headers(&json!({ "headers": { "user-agent": "PluginUA" } }))
        .unwrap();

    assert_eq!(
        headers.get("user-agent").unwrap().to_str().unwrap(),
        "PluginUA"
    );
}

#[test]
fn base64_body_is_decoded_to_binary_bytes() {
    let request = reqwest::Client::new().put("https://example.test/upload");
    let request = apply_body(
        request,
        &json!({ "body_format": "base64" }),
        &json!(BASE64_STANDARD.encode([0, 1, 2, 255])),
    )
    .unwrap()
    .build()
    .unwrap();

    assert_eq!(request.body().unwrap().as_bytes().unwrap(), &[0, 1, 2, 255]);
}

#[tokio::test]
async fn base64_response_format_preserves_binary_bytes() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\n\0\x01\x02\xff")
            .await
            .unwrap();
    });
    let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());

    let result = host
        .http_request(
            &json!({
                "url": format!("http://{address}"),
                "response_body_format": "base64"
            }),
            false,
        )
        .await
        .unwrap();

    assert_eq!(
        result["body_base64"],
        BASE64_STANDARD.encode([0, 1, 2, 255])
    );
    assert_eq!(result["headers"]["content-type"], "image/png");
}

#[tokio::test]
async fn image_inspection_returns_reference_dimensions() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../local-launcher/assets/mediagen.png");
    let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());

    let result = host
        .inspect_image(&json!({ "path": path, "max_bytes": 5 * 1024 * 1024 }))
        .await
        .unwrap();

    assert_eq!(result["width"], 1024);
    assert_eq!(result["height"], 1024);
}
