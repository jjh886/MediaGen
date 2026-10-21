use super::*;

impl ServerPluginHost {
    pub(super) async fn build_request(
        &self,
        params: &Value,
    ) -> Result<reqwest::RequestBuilder, String> {
        let method = params["method"].as_str().unwrap_or("GET");
        let url = required_str(params, "url")?;
        let client = self.client_for(params)?;
        let mut request = client
            .request(
                method
                    .parse()
                    .map_err(|_| format!("不支持的 HTTP 方法: {method}"))?,
                url,
            )
            .headers(self.request_headers(params)?)
            .timeout(Duration::from_millis(
                params["timeout_ms"].as_u64().unwrap_or(30_000),
            ));
        if let Some(body) = params.get("body").filter(|value| !value.is_null()) {
            request = apply_body(request, params, body)?;
        }
        Ok(request)
    }

    pub(crate) fn client_for(&self, params: &Value) -> Result<reqwest::Client, String> {
        let redirects = self.native_redirect_policy(params);
        if params["proxy"] == "direct"
            || params["use_proxy"] == false
            || params["use_account_proxy"] == false
        {
            return http_client_builder(params)
                .no_proxy()
                .redirect(redirects)
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|_| "创建直连客户端失败".to_string());
        }
        let Some(proxy_url) = proxy_url(params, &self.proxy_url) else {
            if params["follow_redirects"] == false
                || self.policy.is_some()
                || params["http1_only"] == true
            {
                return http_client_builder(params)
                    .redirect(redirects)
                    .build()
                    .map_err(|_| "创建 HTTP 客户端失败".to_string());
            }
            return Ok(self.client.clone());
        };
        http_client_builder(params)
            .timeout(Duration::from_secs(30))
            .redirect(redirects)
            .proxy(
                reqwest::Proxy::all(&proxy_url)
                    .map_err(|err| format!("代理地址无效: {}", err.without_url()))?,
            )
            .build()
            .map_err(|err| err.to_string())
    }

    pub(crate) fn request_headers(&self, params: &Value) -> Result<HeaderMap, String> {
        let mut headers = request_headers(params)?;
        if !self.request_id.is_empty() {
            insert_header_if_missing(
                &mut headers,
                HeaderName::from_static("x-client-request-id"),
                self.request_id.clone(),
            )?;
        }
        if params["fingerprint"] == "none" {
            return Ok(headers);
        }
        let Some(fingerprint) = &self.browser_fingerprint else {
            return Ok(headers);
        };
        insert_header_if_missing(&mut headers, USER_AGENT, fingerprint.user_agent())?;
        insert_header_if_missing(
            &mut headers,
            HeaderName::from_static("sec-ch-ua"),
            fingerprint.sec_ch_ua(),
        )?;
        insert_header_if_missing(
            &mut headers,
            HeaderName::from_static("sec-ch-ua-platform"),
            format!("\"{}\"", fingerprint.sec_ch_ua_platform()),
        )?;
        insert_header_if_missing(
            &mut headers,
            HeaderName::from_static("sec-ch-ua-mobile"),
            "?0".to_string(),
        )?;
        Ok(headers)
    }
}

impl ServerPluginHost {
    pub(super) async fn http_request(&self, params: &Value, sse: bool) -> Result<Value, String> {
        let response = self.send_http(params).await?;
        let status = response.status;
        let success = (200..300).contains(&status);
        let headers = response.headers.clone();
        let final_url = response.final_url.clone();
        if params["response_body_format"].as_str() == Some("base64") {
            let bytes = read_response_bounded(response).await?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(format!("HTTP 二进制响应超过限制 {} bytes", MAX_FILE_BYTES));
            }
            let mut result = json!({
                "status": status,
                "body_base64": BASE64_STANDARD.encode(bytes),
                "headers": headers,
                "ok": success,
            });
            if let Some(url) = final_url {
                result["final_url"] = json!(url);
            }
            return Ok(result);
        }
        let body_text =
            String::from_utf8_lossy(&read_response_bounded(response).await?).into_owned();
        let body = serde_json::from_str::<Value>(&body_text)
            .unwrap_or_else(|_| Value::String(body_text.clone()));
        let mut result = json!({
            "status": status,
            "body": body,
            "body_text": body_text,
            "headers": headers,
            "events": if sse { collect_sse_events(&body_text) } else { Vec::<Value>::new() },
            "ok": success,
        });
        if let Some(url) = final_url {
            result["final_url"] = json!(url);
        }
        Ok(result)
    }
}

fn http_client_builder(params: &Value) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    if params["http1_only"] == true {
        builder.http1_only()
    } else {
        builder
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn multipart_file_upload_preserves_bytes_and_final_url_is_opt_in() {
        let root = std::env::temp_dir().join(format!("upload-sync-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("clip.wav");
        let audio = b"RIFF\x00\x01\x02WAVE-data";
        std::fs::write(&path, audio).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for index in 0..5 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut received = Vec::new();
                let header_end = loop {
                    let mut buf = [0; 8192];
                    let count = socket.read(&mut buf).await.unwrap();
                    assert!(count > 0);
                    received.extend_from_slice(&buf[..count]);
                    if let Some(end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let header = String::from_utf8_lossy(&received[..header_end]).to_string();
                if index == 0 {
                    assert!(header
                        .to_lowercase()
                        .contains("content-type: multipart/form-data; boundary="));
                    let length = header
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    while received.len() < header_end + length {
                        let mut buf = [0; 8192];
                        let count = socket.read(&mut buf).await.unwrap();
                        assert!(count > 0);
                        received.extend_from_slice(&buf[..count]);
                    }
                    assert!(received.windows(audio.len()).any(|w| w == audio));
                    assert!(String::from_utf8_lossy(&received).contains("filename=\"clip.wav\""));
                }
                let response = if index == 1 || index == 3 {
                    "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
                };
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let host = ServerPluginHost::new_with_metadata(String::new(), &HashMap::new());
        let request = json!({"method":"POST","url":format!("{origin}/upload"),"proxy":"direct","body_format":"multipart_file","body_file":path,"body_file_field":"file","body_file_name":"clip.wav","body_file_mime":"audio/wav","body_file_size":audio.len()});
        assert_eq!(
            host.execute("http.request", &request).await.unwrap()["ok"],
            true
        );
        let mut invalid = request.clone();
        invalid["body_file_size"] = json!(audio.len() + 1);
        assert!(host
            .execute("http.request", &invalid)
            .await
            .unwrap_err()
            .contains("大小不一致"));
        for include in [false, true] {
            let value=host.execute("http.request",&json!({"url":format!("{origin}/redirect"),"proxy":"direct","include_final_url":include,"response_body_format":"base64"})).await.unwrap();
            if include {
                assert_eq!(value["final_url"], format!("{origin}/final"));
            } else {
                assert!(value.get("final_url").is_none());
            }
        }
        server.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
