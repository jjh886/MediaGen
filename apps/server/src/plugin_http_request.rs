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
            return reqwest::Client::builder()
                .no_proxy()
                .redirect(redirects)
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|_| "创建直连客户端失败".to_string());
        }
        let Some(proxy_url) = proxy_url(params, &self.proxy_url) else {
            if params["follow_redirects"] == false || self.policy.is_some() {
                return reqwest::Client::builder()
                    .redirect(redirects)
                    .build()
                    .map_err(|_| "创建 HTTP 客户端失败".to_string());
            }
            return Ok(self.client.clone());
        };
        reqwest::Client::builder()
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
