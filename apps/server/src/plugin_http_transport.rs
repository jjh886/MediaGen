use super::*;
use futures_util::stream::BoxStream;

pub(super) struct TransportResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: BoxStream<'static, Result<bytes::Bytes, String>>,
}

impl ServerPluginHost {
    pub(super) async fn send_http(&self, params: &Value) -> Result<TransportResponse, String> {
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
        let profile = params["tls_profile"].as_str().unwrap_or(&self.http_profile);
        if profile.is_empty() || profile == "native" {
            let response = self
                .build_request(params)
                .await?
                .send()
                .await
                .map_err(|error| error.without_url().to_string())?;
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
                body: response
                    .bytes_stream()
                    .map(|chunk| chunk.map_err(|error| error.without_url().to_string()))
                    .boxed(),
            });
        }
        let key = json!([
            profile,
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
                .map_err(|_| "未支持的 TLS 指纹配置".to_string())?;
            let emulation = wreq_util::Emulation::builder()
                .profile(profile)
                .headers(false)
                .build();
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
            let client = builder.build().map_err(|_| "TLS 指纹客户端初始化失败")?;
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
            .headers(self.request_headers(params)?)
            .timeout(Duration::from_millis(
                params["timeout_ms"].as_u64().unwrap_or(30_000),
            ));
        if let Some(body) = params.get("body").filter(|body| !body.is_null()) {
            request = profile_body(request, params, body)?;
        }
        let response = request.send().await.map_err(|_| "TLS 指纹请求失败或超时")?;
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
            body: response
                .bytes_stream()
                .map(|chunk| chunk.map_err(|_| "读取 TLS 指纹响应失败".into()))
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
