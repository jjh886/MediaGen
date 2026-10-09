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
        if params["response_body_format"].as_str() == Some("base64") {
            let bytes = read_response_bounded(response).await?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(format!("HTTP 二进制响应超过限制 {} bytes", MAX_FILE_BYTES));
            }
            return Ok(json!({
                "status": status,
                "body_base64": BASE64_STANDARD.encode(bytes),
                "headers": headers,
                "ok": success,
            }));
        }
        let body_text =
            String::from_utf8_lossy(&read_response_bounded(response).await?).into_owned();
        let body = serde_json::from_str::<Value>(&body_text)
            .unwrap_or_else(|_| Value::String(body_text.clone()));
        Ok(json!({
            "status": status,
            "body": body,
            "body_text": body_text,
            "headers": headers,
            "events": if sse { collect_sse_events(&body_text) } else { Vec::<Value>::new() },
            "ok": success,
        }))
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
