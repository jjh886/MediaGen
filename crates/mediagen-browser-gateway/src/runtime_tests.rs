use super::{
    browser_launch_url, local_bundle_candidate_paths, parse_runtime_port, proxy_launch_args,
};
use mediagen_core::{bundled_cloakbrowser_candidates, first_existing_browser_binary};

#[test]
fn local_bundle_resolution_reuses_packaged_runtime_and_prefers_install_directory() {
    let root = std::env::temp_dir().join(format!("mediagen-runtime-{}", uuid::Uuid::new_v4()));
    let installed = bundled_cloakbrowser_candidates(&root).remove(0);
    let packaged =
        bundled_cloakbrowser_candidates(&root.join("dist-local/MediaGen-local")).remove(0);
    let resolve = || first_existing_browser_binary(local_bundle_candidate_paths(&root));
    assert_eq!(resolve(), None);

    std::fs::create_dir_all(packaged.parent().unwrap()).unwrap();
    // 目录不能被误认为浏览器可执行文件。
    std::fs::create_dir(&packaged).unwrap();
    let directory_result = resolve();
    std::fs::remove_dir(&packaged).unwrap();
    std::fs::write(&packaged, b"").unwrap();
    let packaged_result = resolve();
    std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
    std::fs::write(&installed, b"").unwrap();
    let installed_result = resolve();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(directory_result, None);
    assert_eq!(packaged_result, Some(packaged));
    assert_eq!(installed_result, Some(installed));
}

#[test]
fn browser_starts_blank_before_cdp_guards_are_ready() {
    assert_eq!(browser_launch_url(), "about:blank");
}

#[test]
fn proxied_browser_keeps_chromiums_implicit_loopback_bypass() {
    assert_eq!(proxy_launch_args(None), Vec::<String>::new());
    assert_eq!(
        proxy_launch_args(Some("http://127.0.0.1:12345".into())),
        vec!["--proxy-server=http://127.0.0.1:12345"]
    );
}

#[test]
fn runtime_port_parser_rejects_invalid_registry_values() {
    assert_eq!(parse_runtime_port("52500\n"), Some(52500));
    assert_eq!(parse_runtime_port("0"), None);
    assert_eq!(parse_runtime_port("not-a-port"), None);
}

#[tokio::test]
async fn browser_reuse_requires_same_proxy_credentials_and_fingerprint() {
    use crate::{BrowserGateway, BrowserGatewayConfig, BrowserSessionCreateRequest};
    let gateway = BrowserGateway::new(BrowserGatewayConfig::default());
    let mut session = gateway.build_session(BrowserSessionCreateRequest::default());
    let root = std::env::temp_dir().join(format!("browser-registry-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    session.profile_dir = root.to_string_lossy().into();
    session.browser_profile_dir = session.profile_dir.clone();
    let path = super::runtime_port_file(&session.browser_profile_dir);
    std::fs::write(&path, "52500").unwrap();
    assert!(!crate::proxy::runtime_config_matches(&path, &session).await);
    super::write_runtime_port(&session, 52500).await.unwrap();
    assert!(crate::proxy::runtime_config_matches(&path, &session).await);
    assert_eq!(
        parse_runtime_port(&std::fs::read_to_string(&path).unwrap()),
        Some(52500)
    );
    session.metadata.insert(
        "proxy_server".into(),
        serde_json::json!("http://user:secret@proxy.test:8080"),
    );
    assert!(!crate::proxy::runtime_config_matches(&path, &session).await);
    super::write_runtime_port(&session, 52500).await.unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("secret"));
    session
        .metadata
        .insert("proxy_password".into(), serde_json::json!("changed"));
    assert!(!crate::proxy::runtime_config_matches(&path, &session).await);
    super::write_runtime_port(&session, 52500).await.unwrap();
    session
        .metadata
        .insert("browser_version".into(), serde_json::json!("131"));
    assert!(!crate::proxy::runtime_config_matches(&path, &session).await);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
#[ignore = "需 MEDIAGEN_BROWSER_TEST_EXECUTABLE 指定本机浏览器"]
async fn real_browser_identity_matches_page_worker_and_http_headers() {
    use crate::{BrowserGateway, BrowserGatewayConfig, BrowserSessionCreateRequest};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = std::env::temp_dir().join(format!("mediagen-identity-{}", uuid::Uuid::new_v4()));
    let config = BrowserGatewayConfig {
        browser_path: Some(
            std::env::var_os("MEDIAGEN_BROWSER_TEST_EXECUTABLE")
                .unwrap()
                .into(),
        ),
        profile_root: root.clone(),
        default_proxy_server: None,
        ..Default::default()
    };
    let native = super::installed_browser_major_version(&config)
        .await
        .unwrap();
    // 原生版本和模拟版本都必须在页面、Worker、HTTP 中保持一致。
    for major in [native, native + 1] {
        let gateway = BrowserGateway::new(config.clone());
        let mut session = gateway.build_session(BrowserSessionCreateRequest::default());
        session.headless = true;
        session
            .metadata
            .insert("browser_os".into(), json!(std::env::consts::OS));
        session
            .metadata
            .insert("browser_version".into(), json!(major.to_string()));
        let (runtime, target) = super::launch_cloakbrowser(&config, &session).await.unwrap();
        let (client, _) = crate::cdp::CdpClient::connect(&target.websocket_url)
            .await
            .unwrap();
        crate::fingerprint::apply_browser_fingerprint(&client, &session)
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let fixture = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut chunk = [0u8; 4096];
                let n = tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    stream.read(&mut chunk),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(n > 0 && bytes.len() < 32768);
                bytes.extend_from_slice(&chunk[..n]);
            }
            let body = "<!doctype html><title>Identity test</title>";
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
            String::from_utf8(bytes).unwrap()
        });
        client
            .send("Page.navigate", json!({"url":format!("http://{address}/")}))
            .await
            .unwrap();
        let headers = tokio::time::timeout(std::time::Duration::from_secs(10), fixture)
            .await
            .unwrap()
            .unwrap();
        let mut ready = false;
        for _ in 0..40 {
            let value = client
                .send(
                    "Runtime.evaluate",
                    json!({"expression":"document.title", "returnByValue":true}),
                )
                .await
                .unwrap();
            if value["result"]["value"] == "Identity test" {
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(ready);
        let result = client.send("Runtime.evaluate", json!({"awaitPromise":true,"returnByValue":true,"expression":r#"(async()=>{
            const read=async()=>({ua:navigator.userAgent,appVersion:navigator.appVersion,platform:navigator.platform,hints:await navigator.userAgentData.getHighEntropyValues(['fullVersionList','platformVersion','architecture'])});
            const worker=new Worker(URL.createObjectURL(new Blob(['onmessage=async()=>postMessage(await ('+read.toString()+')())'],{type:'text/javascript'})));
            const data=await new Promise((resolve,reject)=>{worker.onmessage=e=>resolve(e.data);worker.onerror=reject;worker.postMessage({})}); worker.terminate();
            return {page:await read(),worker:data};
        })()"#})).await.unwrap();
        runtime.stop().await;
        let value = &result["result"]["value"];
        assert!(
            headers.contains(&format!("Chrome/{major}.")),
            "HTTP UA 版本不一致"
        );
        assert!(
            headers
                .to_ascii_lowercase()
                .contains(&format!("\"chromium\";v=\"{major}\"")),
            "HTTP Client Hints 版本不一致"
        );
        for context in ["page", "worker"] {
            for field in ["ua", "appVersion"] {
                assert!(value[context][field]
                    .as_str()
                    .unwrap()
                    .contains(&format!("Chrome/{major}.")));
            }
            let brands = value[context]["hints"]["fullVersionList"]
                .as_array()
                .unwrap();
            for name in ["Google Chrome", "Chromium"] {
                assert!(brands.iter().any(|brand| brand["brand"] == name
                    && brand["version"]
                        .as_str()
                        .unwrap()
                        .starts_with(&format!("{major}."))));
            }
        }
        assert_eq!(value["page"], value["worker"]);
    }
    std::fs::remove_dir_all(root).unwrap();
}
