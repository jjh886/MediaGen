use super::*;
use futures_util::stream::BoxStream;

pub(super) struct TransportResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub final_url: Option<String>,
    pub body: BoxStream<'static, Result<bytes::Bytes, String>>,
}

impl ServerPluginHost {
    pub(super) async fn send_http(&self, params: &Value) -> Result<TransportResponse, String> {
        let started = std::time::Instant::now();
        let result = self.send_http_inner(params).await;
        let (status, error, headers) = match &result {
            Ok(response) if response.status >= 400 => {
                (Some(response.status), None, Some(&response.headers))
            }
            Err(error) => (
                None,
                Some(crate::protocol_error::sanitize_error(error)),
                None,
            ),
            _ => return result,
        };
        let detail = json!({"event":"plugin_http_failure", "request_id":self.request_id,
            "method":params["method"].as_str().unwrap_or("GET"),
            "url":crate::protocol_error::sanitize_error(params["url"].as_str().unwrap_or("")),
            "status":status, "error":error, "elapsed_ms":started.elapsed().as_millis(),
            "upstream_request_id":headers.and_then(|headers| headers.get("x-request-id")).map(|id| crate::protocol_error::sanitize_error(id)),
            "cf_ray":headers.and_then(|headers| headers.get("cf-ray")).map(|id| crate::protocol_error::sanitize_error(id))});
        eprintln!("{detail}");
        result
    }

    async fn send_http_inner(&self, params: &Value) -> Result<TransportResponse, String> {
        let mut prepared = params.clone();
        if let Some(path) = params["body_file"].as_str() {
            let file = self.read_base64(&json!({"path":path})).await?;
            if params["body_file_size"]
                .as_u64()
                .is_some_and(|n| file["size"].as_u64() != Some(n))
            {
                return Err("上传文件大小不一致".into());
            }
            if params["body_format"] == "multipart_file" {
                let field = params["body_file_field"].as_str().unwrap_or("file");
                prepared["body_format"] = json!("multipart");
                prepared["body"] = params["body"]
                    .as_object()
                    .cloned()
                    .map(Value::Object)
                    .unwrap_or(json!({}));
                prepared["body"][field] = json!({"base64":file["base64"],"filename":params["body_file_name"].as_str().unwrap_or("upload"),"mime_type":params["body_file_mime"].as_str().unwrap_or("application/octet-stream")});
            } else {
                prepared["body_format"] = json!("base64");
                prepared["body"] = file["base64"].clone();
            }
        }
        let params = &prepared;
        let mutating = matches!(
            params["method"]
                .as_str()
                .unwrap_or("GET")
                .to_uppercase()
                .as_str(),
            "POST" | "PUT" | "PATCH" | "DELETE"
        );
        let previous = self.submitted.load(std::sync::atomic::Ordering::Relaxed);
        if mutating {
            self.submitted
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let headers = self.request_headers(params)?;
        let configured = params["tls_profile"].as_str().unwrap_or(&self.http_profile);
        let profile = if self.http_profile == "browser" || configured == "browser" {
            let major = headers
                .get(USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .and_then(|ua| ua.split("Chrome/").nth(1))
                .and_then(|version| version.split('.').next())
                .and_then(|major| major.parse::<u16>().ok())
                .ok_or("浏览器 API 缺少有效 Chrome 指纹，请配置账号浏览器环境")?;
            format!(
                "chrome_{}",
                crate::account_browser_profile::compatible_browser_version(major)
            )
        } else {
            configured.to_string()
        };
        let platform = match headers
            .get("sec-ch-ua-platform")
            .and_then(|v| v.to_str().ok())
        {
            Some("\"Windows\"") => wreq_util::Platform::Windows,
            Some("\"Linux\"") => wreq_util::Platform::Linux,
            _ => wreq_util::Platform::MacOS,
        };
        if profile.is_empty() || profile == "native" {
            let response = self
                .build_request(params)
                .await?
                .send()
                .await
                .map_err(|error| crate::protocol_error::error_chain(&error.without_url()))?;
            if mutating
                && (400..500).contains(&response.status().as_u16())
                && response.status().as_u16() != 408
            {
                self.submitted
                    .store(previous, std::sync::atomic::Ordering::Relaxed);
            }
            return Ok(TransportResponse {
                status: response.status().as_u16(),
                headers: response_headers(response.headers()),
                final_url: (params["include_final_url"] == true)
                    .then(|| response.url().to_string()),
                body: response
                    .bytes_stream()
                    .map(|chunk| {
                        chunk.map_err(|error| {
                            crate::protocol_error::error_chain(&error.without_url())
                        })
                    })
                    .boxed(),
            });
        }
        let key = json!([
            profile,
            format!("{platform:?}"),
            proxy_url(params, &self.proxy_url),
            params["proxy"],
            params["use_proxy"],
            params["use_account_proxy"],
            params["follow_redirects"]
        ])
        .to_string();
        let cached = self
            .profile_clients
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
            .cloned();
        let client = if let Some(client) = cached {
            client
        } else {
            let profile: wreq_util::Profile = serde_json::from_value(json!(profile))
                .map_err(|_| "TLS 指纹配置无效".to_string())?;
            use wreq::IntoEmulation;
            let mut emulation = wreq_util::Emulation::builder()
                .profile(profile)
                .platform(platform)
                .build()
                .into_emulation();
            for name in emulation.headers.keys() {
                emulation.orig_headers.insert(name.as_str().to_string());
            }
            let mut builder = wreq::Client::builder()
                .emulation(emulation)
                .connect_timeout(Duration::from_secs(30));
            if params["proxy"] == "direct"
                || params["use_proxy"] == false
                || params["use_account_proxy"] == false
            {
                builder = builder.no_proxy();
            } else if let Some(proxy) = proxy_url(params, &self.proxy_url) {
                builder = builder.proxy(wreq::Proxy::all(&proxy).map_err(|_| "代理配置无效")?);
            }
            builder = builder.redirect(self.profile_redirect_policy(params));
            let client = builder
                .build()
                .map_err(|error| crate::protocol_error::error_chain(&error.without_uri()))?;
            let mut clients = self
                .profile_clients
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if clients.len() >= 8 {
                clients.clear();
            }
            clients.insert(key, client.clone());
            client
        };
        let mut request = client
            .request(
                params["method"]
                    .as_str()
                    .unwrap_or("GET")
                    .parse::<wreq::Method>()
                    .map_err(|_| "HTTP 方法无效")?,
                required_str(params, "url")?,
            )
            .headers(headers)
            .timeout(Duration::from_millis(
                params["timeout_ms"].as_u64().unwrap_or(30_000),
            ));
        if let Some(body) = params.get("body").filter(|body| !body.is_null()) {
            request = profile_body(request, params, body)?;
        }
        let response = request
            .send()
            .await
            .map_err(|error| crate::protocol_error::error_chain(&error.without_uri()))?;
        if mutating
            && (400..500).contains(&response.status().as_u16())
            && response.status().as_u16() != 408
        {
            self.submitted
                .store(previous, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(TransportResponse {
            status: response.status().as_u16(),
            headers: response_headers(response.headers()),
            final_url: (params["include_final_url"] == true).then(|| response.uri().to_string()),
            body: response
                .bytes_stream()
                .map(|chunk| {
                    chunk.map_err(|error| crate::protocol_error::error_chain(&error.without_uri()))
                })
                .boxed(),
        })
    }
}

fn profile_body(
    request: wreq::RequestBuilder,
    params: &Value,
    body: &Value,
) -> Result<wreq::RequestBuilder, String> {
    match params["body_format"].as_str().unwrap_or("json") {
        "multipart" => {
            let mut form = wreq::multipart::Form::new();
            for (name, value) in body.as_object().ok_or("multipart body 必须是对象")? {
                if let Some(encoded) = value["base64"].as_str() {
                    let bytes = BASE64_STANDARD
                        .decode(encoded)
                        .map_err(|_| "上传文件编码无效")?;
                    if bytes.len() > MAX_FILE_BYTES as usize {
                        return Err("上传文件过大".into());
                    }
                    let part = wreq::multipart::Part::bytes(bytes)
                        .file_name(value["filename"].as_str().unwrap_or("upload").to_string())
                        .mime_str(
                            value["mime_type"]
                                .as_str()
                                .unwrap_or("application/octet-stream"),
                        )
                        .map_err(|_| "文件 MIME 类型无效")?;
                    form = form.part(name.clone(), part);
                } else {
                    form = form.text(name.clone(), value_to_string(value));
                }
            }
            Ok(request.multipart(form))
        }
        "form" | "x-www-form-urlencoded" => {
            let pairs = body
                .as_object()
                .ok_or("form body 必须是对象")?
                .iter()
                .map(|(key, value)| (key.clone(), value_to_string(value)))
                .collect::<Vec<_>>();
            Ok(request.form(&pairs))
        }
        "base64" => Ok(request.body(
            BASE64_STANDARD
                .decode(body.as_str().ok_or("body 必须是字符串")?)
                .map_err(|_| "body 编码无效")?,
        )),
        "text" | "raw" => Ok(request.body(value_to_string(body))),
        _ => Ok(request.json(body)),
    }
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn browser_profiles_align_credentials_and_headers_but_native_clients_keep_identity() {
        use mediagen_plugin_runtime::PluginHost;
        let metadata = HashMap::from([
            ("browser_os".into(), json!("windows")),
            ("browser_version".into(), json!("999")),
        ]);
        let mut host = ServerPluginHost::new_with_metadata(String::new(), &metadata);
        let manifest = serde_json::from_value(
            json!({"platform_id":"test","display_name":"Test","login_credential_capture":{},"protected_json_files":["auth.json"]}),
        )
        .unwrap();
        host = host.with_http_profile(&manifest);
        let headers = host
            .request_headers(
                &json!({"headers":{"user-agent":"Chrome/149.0.0.0","sec-ch-ua-platform":"\"macOS\""}}),
            )
            .unwrap();
        let latest = crate::account_browser_profile::supported_browser_versions()[0];
        assert!(headers["user-agent"]
            .to_str()
            .unwrap()
            .contains(&format!("Chrome/{latest}.")));
        assert_eq!(headers["sec-ch-ua-platform"], "\"Windows\"");
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("auth.json");
        let original = json!({"auth":{"fingerprint":{"user_agent":"Chrome/149.0.0.0","hardware_concurrency":8,"languages":["zh-CN","zh"]}},"cookies":[]});
        std::fs::write(&path, original.to_string()).unwrap();
        let cipher = std::sync::Arc::new(
            crate::generation_queue::CredentialCipher::new(root.path()).unwrap(),
        );
        host = host.with_storage(cipher.clone(), root.path().to_str().unwrap(), &manifest);
        for _ in 0..2 {
            let result = host.call("file.read_json", &json!({"path":path})).await;
            assert!(result.success, "{:?}", result.error);
            assert_eq!(
                result.data["auth"]["fingerprint"]["user_agent"],
                headers["user-agent"].to_str().unwrap()
            );
            assert_eq!(result.data["auth"]["fingerprint"]["platform"], "Win32");
            assert_eq!(
                result.data["auth"]["fingerprint"]["hardware_concurrency"],
                8
            );
        }
        let headers = host.request_headers(&json!({})).unwrap();
        assert_eq!(headers["accept-language"], "zh-CN,zh;q=0.9");
        assert_eq!(
            headers["sec-ch-ua-full-version"],
            format!("\"{latest}.0.0.0\"")
        );
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let plain: Value = serde_json::from_str(
            &cipher
                .decrypt(stored["ciphertext"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(plain, original);
        let legacy = ServerPluginHost::new_with_metadata(String::new(), &HashMap::new())
            .with_http_profile(&manifest)
            .with_storage(cipher, root.path().to_str().unwrap(), &manifest);
        assert!(
            legacy
                .call("file.read_json", &json!({"path":path}))
                .await
                .success
        );
        assert!(legacy.request_headers(&json!({})).unwrap()["user-agent"]
            .to_str()
            .unwrap()
            .contains("Chrome/149."));
        host.http_profile = "native".into();
        let headers = host
            .request_headers(&json!({"headers":{"user-agent":"NativeClient/1.0"}}))
            .unwrap();
        assert_eq!(headers["user-agent"], "NativeClient/1.0");
        assert!(!headers.contains_key("sec-ch-ua"));
    }

    #[tokio::test]
    async fn account_proxy_carries_version_specific_tls_client_hello() {
        for version in [99, 149, 150, 999] {
            let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = proxy.local_addr().unwrap();
            let capture = tokio::spawn(async move {
                let (mut stream, _) = proxy.accept().await.unwrap();
                let mut connect = Vec::new();
                while !connect.ends_with(b"\r\n\r\n") {
                    connect.push(stream.read_u8().await.unwrap());
                    assert!(connect.len() < 8192);
                }
                assert!(String::from_utf8_lossy(&connect).starts_with("CONNECT example.test:443 "));
                stream
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .unwrap();
                let mut record = [0; 5];
                stream.read_exact(&mut record).await.unwrap();
                assert_eq!(record[0], 22);
                let mut hello = vec![0; u16::from_be_bytes([record[3], record[4]]) as usize];
                stream.read_exact(&mut hello).await.unwrap();
                hello
            });
            let metadata = HashMap::from([
                ("browser_os".into(), json!("windows")),
                ("browser_version".into(), json!(version.to_string())),
            ]);
            let manifest = serde_json::from_value(
                json!({"platform_id":"test","display_name":"Test","login_credential_capture":{},"protected_json_files":["auth.json"]}),
            )
            .unwrap();
            let host = ServerPluginHost::new_with_metadata(format!("http://{address}"), &metadata)
                .with_http_profile(&manifest);
            let error = host
                .send_http(&json!({"url":"https://example.test/", "timeout_ms":2000}))
                .await
                .err()
                .unwrap();
            assert!(
                error.contains("caused by:"),
                "必须保留实际 TLS 错误链: {error}"
            );
            let hello = tokio::time::timeout(Duration::from_secs(3), capture)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(hello[0], 1);
            let extensions = extensions(&hello);
            assert!(extensions[&16].windows(3).any(|value| value == b"\x02h2"));
            let algorithms = &extensions[&13];
            let modern = algorithms[2..].as_chunks::<2>().0.contains(&[9, 4]);
            assert_eq!(
                modern,
                version >= 150,
                "Chrome {version} 必须实际改变 TLS 签名算法"
            );
        }
    }

    fn extensions(hello: &[u8]) -> HashMap<u16, Vec<u8>> {
        let number = |offset| u16::from_be_bytes([hello[offset], hello[offset + 1]]) as usize;
        let mut cursor = 38;
        cursor += 1 + hello[cursor] as usize;
        cursor += 2 + number(cursor);
        cursor += 1 + hello[cursor] as usize;
        let end = cursor + 2 + number(cursor);
        cursor += 2;
        let mut result = HashMap::new();
        while cursor < end {
            let kind = number(cursor) as u16;
            let length = number(cursor + 2);
            cursor += 4;
            result.insert(kind, hello[cursor..cursor + length].to_vec());
            cursor += length;
        }
        result
    }
}
