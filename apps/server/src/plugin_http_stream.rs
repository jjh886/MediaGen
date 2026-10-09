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
        self.validate_policy("http.sse_request", params)?;
        let response = tokio::select! {
            _ = events.closed() => return Err("请求已取消".into()),
            response = self.send_http(params) => response?,
        };
        let status = response.status;
        let ok = (200..300).contains(&status);
        let headers = response.headers.clone();
        if params["include_http_metadata"] == true {
            events
                .send(json!({"type":"http_response","status":status,"ok":ok,"headers":headers}))
                .await
                .map_err(|_| "请求已取消")?;
        }
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
                return Ok(json!({"status":status,"ok":true,"headers":headers,"body_text":text}));
            }
            return Ok(
                json!({"status":status,"ok":false,"headers":headers,"body":body,"body_text":text}),
            );
        }
        let mut collected = Vec::new();
        let metadata = params["include_sse_metadata"] == true;
        let mut stream = response.body;
        let mut decoder = crate::sse_decoder::Decoder::new(MAX_FILE_BYTES as usize);
        loop {
            let chunk = tokio::select! {
                _ = events.closed() => return Err("请求已取消".into()),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk?;
            if params["collect_body"] == true {
                if collected.len() + chunk.len() > MAX_FILE_BYTES as usize {
                    return Err("事件流正文超过大小限制".into());
                }
                collected.extend_from_slice(&chunk);
            }
            for block in decoder.push(&chunk)? {
                let stop = params["stop_condition"].as_str().is_some_and(|condition| {
                    std::str::from_utf8(&block)
                        .ok()
                        .and_then(parse_sse_block)
                        .is_some_and(|value| matches_condition(&value, condition))
                });
                forward_event(&events, &block, metadata).await?;
                if stop {
                    return Ok(
                        json!({"status":status,"ok":true,"headers":headers,"body_text":String::from_utf8_lossy(&collected)}),
                    );
                }
            }
        }
        forward_event(&events, &decoder.finish(), metadata).await?;
        Ok(
            json!({"status":status,"ok":true,"headers":headers,"body_text":String::from_utf8_lossy(&collected)}),
        )
    }
}

async fn forward_event(
    events: &tokio::sync::mpsc::Sender<Value>,
    bytes: &[u8],
    metadata: bool,
) -> Result<(), String> {
    let block = std::str::from_utf8(bytes).map_err(|_| "事件流包含无效 UTF-8".to_string())?;
    if let Some(value) = parse_event(block, metadata) {
        events
            .send(value)
            .await
            .map_err(|_| "请求已取消".to_string())?;
    }
    Ok(())
}

fn matches_condition(data: &Value, condition: &str) -> bool {
    let (path, expected, negate) = if let Some((path, value)) = condition.split_once("!=") {
        (path, value, true)
    } else if let Some((path, value)) = condition.split_once("==") {
        (path, value, false)
    } else {
        return false;
    };
    let actual = path
        .trim()
        .split('.')
        .fold(data, |value, key| value.get(key).unwrap_or(&Value::Null));
    let text = match actual {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    (text == expected.trim().trim_matches('\'').trim_matches('"')) != negate
}

fn parse_event(block: &str, metadata: bool) -> Option<Value> {
    if !metadata {
        return parse_sse_block(block);
    }
    let mut event = "message";
    let mut id = None;
    let mut data = Vec::new();
    for line in block.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = value.trim();
        }
        if let Some(value) = line.strip_prefix("id:") {
            id = Some(value.trim());
        }
        if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    if data.is_empty() {
        return None;
    }
    let text = data.join("\n");
    let body = serde_json::from_str::<Value>(&text).unwrap_or(json!(text));
    let mut value = json!({"event":event,"data":body});
    if let Some(id) = id {
        value["id"] = json!(id);
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_events_preserve_provider_metadata_without_changing_legacy_events() {
        let block = "event: ready\nid: 7\ndata: {\"response_message_id\":42}";
        assert_eq!(
            parse_event(block, true).unwrap(),
            json!({"event":"ready","id":"7","data":{"response_message_id":42}})
        );
        assert_eq!(
            parse_event(block, false).unwrap(),
            json!({"response_message_id":42})
        );
        assert!(parse_event(": heartbeat", true).is_none());
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn stop_condition_delivers_the_terminal_event_without_waiting_for_connection_close() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            assert!(socket.read(&mut bytes).await.unwrap() > 0);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\ndata: {\"type\":\"delta\",\"text\":\"ok\"}\n\ndata: {\"type\":\"finished\"}\n\ndata: {\"type\":\"ignored\"}\n\n").await.unwrap();
            std::future::pending::<()>().await;
        });
        let host = ServerPluginHost::new_with_metadata(String::new(), &HashMap::new());
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        let result = tokio::time::timeout(Duration::from_secs(2),host.stream_request(&json!({"url":format!("http://{address}/"),"proxy":"direct","stop_condition":"type == 'finished'"}),sender)).await.unwrap().unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(receiver.recv().await.unwrap()["type"], "delta");
        assert_eq!(receiver.recv().await.unwrap()["type"], "finished");
        assert!(receiver.recv().await.is_none());
        server.abort();
    }
    #[test]
    fn conditions_compare_nested_values_without_executing_expressions() {
        assert!(matches_condition(
            &json!({"data":{"status":2}}),
            "data.status == '2'"
        ));
        assert!(!matches_condition(
            &json!({"data":{"status":2}}),
            "data.status != 2"
        ));
        assert!(!matches_condition(&json!({}), "throw new Error()"));
    }
}
