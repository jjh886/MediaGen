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

#[tokio::test]
async fn login_proxy_credentials_and_generic_crypto_atoms_are_preserved() {
    use mediagen_plugin_runtime::PluginHost;
    let metadata = HashMap::from([
        ("proxy_username".into(), json!("u@a")),
        ("proxy_password".into(), json!("p:a/ss")),
    ]);
    let host = ServerPluginHost::new_with_metadata("socks5h://proxy.test:1080".into(), &metadata);
    assert_eq!(host.proxy_url, "socks5h://u%40a:p%3Aa%2Fss@proxy.test:1080");
    let result = host.call("crypto.random_uuid", &json!({})).await;
    let id = uuid::Uuid::parse_str(result.data["value"].as_str().unwrap()).unwrap();
    assert_eq!(id.get_version_num(), 4);
    let digest = host.call("crypto.sha256_hex", &json!({"text":"abc"})).await;
    assert_eq!(
        digest.data["value"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
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

#[tokio::test]
async fn http_stream_forwards_event_before_upstream_finishes() {
    use mediagen_plugin_runtime::PluginHost;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (finish, release) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"delta\":\"first\"}\n\n").await.unwrap();
        let _ = release.await;
        socket
            .write_all(b"data: {\"delta\":\"last\"}\n\n")
            .await
            .unwrap();
    });
    let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
    let (sender, mut events) = tokio::sync::mpsc::channel(2);
    let task = tokio::spawn(async move {
        host.stream(
            "http.sse_request",
            &json!({"url":format!("http://{address}"),"proxy":"direct"}),
            sender,
        )
        .await
    });
    let first = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first["delta"], "first");
    assert!(!task.is_finished());
    finish.send(()).unwrap();
    assert_eq!(events.recv().await.unwrap()["delta"], "last");
    assert!(task.await.unwrap().success);
}

#[tokio::test]
async fn emulated_transport_preserves_binary_and_multiple_cookies() {
    use mediagen_plugin_runtime::PluginHost;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nSet-Cookie: first=1; Path=/\r\nSet-Cookie: second=2; Path=/\r\nContent-Length: 3\r\n\r\nID3").await.unwrap();
    });
    let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
    let result = host.call("http.request", &json!({"url":format!("http://{address}"),"proxy":"direct","tls_profile":"chrome_149","response_body_format":"base64"})).await;
    assert!(result.success, "{:?}", result.error);
    assert_eq!(result.data["body_base64"], "SUQz");
    assert!(result.data["headers"]["set-cookie"]
        .as_str()
        .unwrap()
        .contains("first=1; Path=/\nsecond=2; Path=/"));
}

#[test]
fn non_browser_identity_does_not_receive_browser_hints() {
    let metadata = HashMap::from([
        ("browser_os".into(), json!("windows")),
        ("browser_version".into(), json!("126")),
    ]);
    let host = ServerPluginHost::new_with_metadata(String::new(), &metadata)
        .with_events(None, "request-123".into());
    let headers = host
        .request_headers(&json!({"fingerprint":"none","headers":{"user-agent":"NativeClient"}}))
        .unwrap();
    assert!(!headers.contains_key("sec-ch-ua"));
    assert_eq!(headers["x-client-request-id"], "request-123");
}

#[tokio::test]
async fn encrypted_credentials_and_checkpoints_survive_new_runtime() {
    use mediagen_plugin_runtime::{run_js_step, PluginHost};
    let root = std::env::temp_dir().join(format!("secure-host-{}", uuid::Uuid::new_v4()));
    let cipher =
        std::sync::Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
    let manifest: mediagen_core::PluginManifest = serde_json::from_value(
        json!({"platform_id":"test","display_name":"Test","protected_json_files":["auth.json"]}),
    )
    .unwrap();
    let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default())
        .with_storage(cipher.clone(), root.to_str().unwrap(), &manifest)
        .with_events(None, "persistent-task".into());
    let path = root.join("auth.json");
    std::fs::write(&path, r#"{"token":"secret-token"}"#).unwrap();
    assert_eq!(
        host.call("file.read_json", &json!({"path":path}))
            .await
            .data["token"],
        "secret-token"
    );
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("secret-token"));
    run_js_step("function main(){host.checkpoint({phase:'accepted',conversation_id:'upstream-id'});return {}}".into(), json!({}), host).await.unwrap();
    let restored = ServerPluginHost::new_with_metadata(String::new(), &Default::default())
        .with_storage(cipher, root.to_str().unwrap(), &manifest)
        .with_events(None, "persistent-task".into());
    let result = run_js_step(
        "function main(){return host.checkpoint()}".into(),
        json!({}),
        restored.clone(),
    )
    .await
    .unwrap();
    assert_eq!(result["conversation_id"], "upstream-id");
    assert!(!restored.safe_to_retry().await);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn native_and_fingerprint_transports_preserve_upload_encodings() {
    use mediagen_plugin_runtime::PluginHost;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for profile in ["native", "chrome_149"] {
        for (format, body, expected) in [
            ("json", json!({"input":"hello"}), "hello"),
            (
                "form",
                json!({"input":"hello world","count":2}),
                "hello+world",
            ),
            ("text", json!("literal body"), "literal body"),
            ("base64", json!("YmluYXJ5"), "binary"),
            (
                "multipart",
                json!({"model":"audio","file":{"base64":"Uk lGRg==".replace(' ',""),"filename":"sample.wav","mime_type":"audio/wav"}}),
                "sample.wav",
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut captured = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    captured.extend_from_slice(&buffer[..count]);
                    if let Some(header_end) =
                        captured.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                    {
                        let headers =
                            String::from_utf8_lossy(&captured[..header_end]).to_lowercase();
                        let length = headers.lines().find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        });
                        if length.is_some_and(|size| captured.len() >= header_end + 4 + size)
                            || captured.ends_with(b"0\r\n\r\n")
                        {
                            break;
                        }
                    }
                }
                assert!(String::from_utf8_lossy(&captured).contains(expected));
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 17\r\nConnection: close\r\n\r\n{\"accepted\":true}").await.unwrap();
            });
            let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
            let result = host.call("http.request", &json!({"method":"POST","url":format!("http://{address}/upload"),"proxy":"direct","tls_profile":profile,"body_format":format,"body":body})).await;
            assert!(result.success, "{:?}", result.error);
            assert_eq!(result.data["body"]["accepted"], true);
            server.await.unwrap();
            assert!(!host.safe_to_retry().await);
        }
    }
}

#[tokio::test]
async fn file_primitives_validate_sizes_and_real_image_headers() {
    use base64::Engine;
    use mediagen_plugin_runtime::PluginHost;
    let root = std::env::temp_dir().join(format!("file-atoms-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("pixel.png");
    image::RgbImage::new(2, 3).save(&path).unwrap();
    let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
    let result = host.call("file.read_base64", &json!({"path":path})).await;
    assert!(result.success);
    let encoded = result.data["base64"].as_str().unwrap();
    assert_eq!(
        base64::prelude::BASE64_STANDARD.decode(encoded).unwrap(),
        std::fs::read(&path).unwrap()
    );
    for params in [json!({"path":path}), json!({"base64":encoded})] {
        let inspected = host.call("file.inspect_image", &params).await;
        assert!(inspected.success);
        assert_eq!(inspected.data["width"], 2);
        assert_eq!(inspected.data["height"], 3);
    }
    assert!(
        !host
            .call("file.read_base64", &json!({"path":path,"max_bytes":1}))
            .await
            .success
    );
    assert!(
        !host
            .call("file.inspect_image", &json!({"base64":"not-base64!"}))
            .await
            .success
    );
    assert!(
        !host
            .call("file.read_json", &json!({"path":root.join("missing.json")}))
            .await
            .success
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// 手动真实账号诊断：只读账号快照，不创建服务、不写业务存储；结果写入调用者指定的私有文件。
#[tokio::test]
#[ignore = "需要显式提供真实账号诊断配置"]
async fn live_plugin_diagnostic() {
    use serde_json::Value;
    use std::path::{Path, PathBuf};
    let path = std::env::var("MEDIAGEN_LIVE_PROBE").expect("缺少诊断配置");
    let config: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let root = PathBuf::from(config["root"].as_str().unwrap());
    let snapshot =
        crate::store_persistence::read_store_snapshot(&root.join("data/mediagen.sqlite"));
    let account = snapshot
        .accounts
        .into_iter()
        .find(|account| account.id == config["account_id"])
        .unwrap();
    let loader = mediagen_plugin_runtime::PluginLoader::load_from(&root.join("plugins")).unwrap();
    let manifest = loader.get_plugin(&account.platform).unwrap();
    let step = &manifest.flows[config["flow"].as_str().unwrap()].steps[0];
    let references = step["script_files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .map(|file| file.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![step["script_file"].as_str().unwrap().into()]);
    let directory = loader.get_plugin_dir(&account.platform).unwrap();
    let mut script = crate::plugin_scripts::read_plugin_scripts(directory, &references).unwrap();
    if let Some(path) = config["inspect_script"].as_str() {
        script.push_str(&std::fs::read_to_string(path).unwrap());
    }
    let cipher = std::sync::Arc::new(
        crate::generation_queue::CredentialCipher::new(&root.join("data")).unwrap(),
    );
    let id = format!("diagnostic-{}", uuid::Uuid::new_v4());
    let host = ServerPluginHost::new_for_account(&account)
        .with_storage(cipher, &account.profile_dir, manifest)
        .with_http_profile(manifest)
        .with_events(None, id.clone());
    // 复测长任务时恢复已有检查点，避免再次提交并消耗上游额度。
    if let Some(model) = config["resume_model"].as_str() {
        let mut files = std::fs::read_dir(Path::new(&account.profile_dir).join("checkpoints"))
            .unwrap()
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        files.sort_by_key(|entry| std::cmp::Reverse(entry.metadata().unwrap().modified().unwrap()));
        let mut resumed = false;
        for file in files {
            let value = host.read_json(&json!({"path":file.path()})).await.unwrap();
            if value["model"] == model && value["conversation_id"].is_string() {
                host.checkpoint(&json!({"value":value})).await.unwrap();
                resumed = true;
                break;
            }
        }
        assert!(resumed, "没有可恢复的检查点，拒绝重复提交");
    }
    let context = json!({"account":account,"account_id":account.id,"profile_dir":account.profile_dir,"scripts_dir":directory.join("scripts"),"request":config["request"],"request_id":id,"task_id":id,"platform_config":snapshot.settings.platform_generation_config(&account.platform)});
    let result = mediagen_plugin_runtime::run_js_step(script, context, host).await;
    let output = match result {
        Ok(value) => json!({"ok":true,"result":value}),
        Err(message) => json!({"ok":false,"message":message}),
    };
    crate::account_login_credentials::write_private_file(
        Path::new(config["output"].as_str().unwrap()),
        output.to_string().as_bytes(),
    )
    .await
    .unwrap();
    println!("diagnostic completed: success={}", output["ok"]);
}

#[tokio::test]
async fn streaming_transport_delivers_json_fallback_to_plugin() {
    use mediagen_plugin_runtime::PluginHost;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        let body = r#"{"created":123,"data":[{"b64_json":"encoded-image"}]}"#;
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let response = host
        .stream(
            "http.sse_request",
            &json!({"url":format!("http://{address}"),"proxy":"direct"}),
            sender,
        )
        .await;
    assert!(response.success);
    assert_eq!(
        receiver.recv().await.unwrap()["data"][0]["b64_json"],
        "encoded-image"
    );
    server.await.unwrap();
}
