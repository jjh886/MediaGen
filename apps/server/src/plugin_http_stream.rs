use super::*;

pub(super) async fn read_response_bounded(
    mut response: super::http_transport::TransportResponse,
) -> Result<Vec<u8>, String> {
    if response
        .headers
        .get("content-length")
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|size| size > MAX_FILE_BYTES)
    {
        return Err("HTTP 响应超过大小限制".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.body.next().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > MAX_FILE_BYTES as usize {
            return Err("HTTP 响应超过大小限制".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

impl ServerPluginHost {
    pub(super) async fn stream_request(
        &self,
        params: &Value,
        events: tokio::sync::mpsc::Sender<Value>,
    ) -> Result<Value, String> {
        let response = tokio::select! {
            _ = events.closed() => return Err("请求已取消".into()),
            response = self.send_http(params) => response?,
        };
        let status = response.status;
        let ok = (200..300).contains(&status);
        let headers = response.headers.clone();
        let json_response = headers.get("content-type").is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().ends_with("json"))
        });
        if !ok || json_response {
            let bytes = read_response_bounded(response).await?;
            let text = String::from_utf8_lossy(&bytes);
            let body =
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::String(text.to_string()));
            if ok {
                if !body.is_object() {
                    return Err("上游 JSON 响应不是有效对象".into());
                }
                events.send(body).await.map_err(|_| "请求已取消")?;
                return Ok(json!({"status":status,"ok":true,"headers":headers}));
            }
            return Ok(
                json!({"status":status,"ok":false,"headers":headers,"body":body,"body_text":text}),
            );
        }
        let mut stream = response.body;
        let mut decoder = crate::sse_decoder::Decoder::new(MAX_FILE_BYTES as usize);
        loop {
            let chunk = tokio::select! {
                _ = events.closed() => return Err("请求已取消".into()),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            for block in decoder.push(&chunk?)? {
                forward_event(&events, &block).await?;
            }
        }
        forward_event(&events, &decoder.finish()).await?;
        Ok(json!({"status":status,"ok":true,"headers":headers}))
    }
}

async fn forward_event(
    events: &tokio::sync::mpsc::Sender<Value>,
    bytes: &[u8],
) -> Result<(), String> {
    let block = std::str::from_utf8(bytes).map_err(|_| "事件流包含无效 UTF-8".to_string())?;
    if let Some(value) = parse_sse_block(block) {
        events
            .send(value)
            .await
            .map_err(|_| "请求已取消".to_string())?;
    }
    Ok(())
}
