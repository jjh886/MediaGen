//! 通用 WebSocket 连接保存在本次插件宿主中，结束执行后随宿主释放。
use super::*;
use crate::realtime_transport::{connect_with_host, Socket};
use futures_util::SinkExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Default)]
pub(super) struct Connections(Arc<Mutex<HashMap<String, Arc<Mutex<Socket>>>>>);

impl ServerPluginHost {
    pub(super) async fn websocket(&self, params: &Value, action: &str) -> Result<Value, String> {
        if action == "http.websocket_open" {
            let url = reqwest::Url::parse(&required_str(params, "url")?)
                .map_err(|_| "WebSocket 地址无效")?;
            if url.scheme() != "wss"
                && !(url.scheme() == "ws" && url.host_str() == Some("127.0.0.1"))
            {
                return Err("WebSocket 需要 WSS 地址".into());
            }
            if !url.username().is_empty() || url.password().is_some() {
                return Err("WebSocket URL 不允许嵌入凭据".into());
            }
            let mut connections = self.sockets.0.lock().await;
            if connections.len() >= 64 {
                return Err("WebSocket 连接数量已达上限".into());
            }
            let socket = connect_with_host(self, params, 16 << 20).await?;
            let id = uuid::Uuid::new_v4().to_string();
            connections.insert(id.clone(), Arc::new(Mutex::new(socket)));
            let weak = Arc::downgrade(&self.sockets.0);
            let expired = id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(30 * 60)).await;
                if let Some(sockets) = weak.upgrade() {
                    sockets.lock().await.remove(&expired);
                }
            });
            return Ok(json!({"connection_id":id}));
        }
        let id = required_str(params, "connection_id")?;
        let socket = {
            let mut sockets = self.sockets.0.lock().await;
            let socket = sockets
                .get(&id)
                .ok_or("WebSocket 连接已关闭或不属于当前插件执行")?
                .clone();
            if action == "http.websocket_close" {
                sockets.remove(&id);
            }
            socket
        };
        let mut socket = socket.lock().await;
        match action {
            "http.websocket_close" => {
                let _ = tokio::time::timeout(Duration::from_secs(2), socket.close(None)).await;
                Ok(json!({"closed":true}))
            }
            "http.websocket_send" => {
                let text = required_str(params, "text")?;
                if text.len() > 1 << 20 {
                    return Err("WebSocket 消息超过 1 MiB".into());
                }
                tokio::time::timeout(
                    Duration::from_secs(15),
                    socket.send(Message::Text(text.into())),
                )
                .await
                .map_err(|_| "WebSocket 发送超时")?
                .map_err(|_| "WebSocket 发送失败")?;
                Ok(json!({"sent":true}))
            }
            "http.websocket_receive" => {
                let ms = params["timeout_ms"]
                    .as_u64()
                    .unwrap_or(1000)
                    .clamp(1, 60000);
                let result = tokio::time::timeout(Duration::from_millis(ms), async {
                    loop {
                        match socket.next().await {
                            Some(Ok(Message::Text(text))) => {
                                return Ok(json!({"text":text.as_str()}))
                            }
                            Some(Ok(Message::Ping(_))) => socket
                                .flush()
                                .await
                                .map_err(|_| "WebSocket 心跳失败".to_string())?,
                            Some(Ok(Message::Close(_))) | None => {
                                return Ok(json!({"closed":true}))
                            }
                            Some(Err(_)) => return Err("WebSocket 读取失败".to_string()),
                            _ => {}
                        }
                    }
                })
                .await;
                match result {
                    Err(_) => Ok(json!({"timeout":true})),
                    Ok(value) => {
                        if value.as_ref().map_or(true, |v| v["closed"] == true) {
                            self.sockets.0.lock().await.remove(&id);
                        }
                        value
                    }
                }
            }
            _ => Err("未知 WebSocket 操作".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn sockets_use_http1_handle_ping_and_are_isolated_between_executions() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                    assert_eq!(request.version(), reqwest::Version::HTTP_11);
                    Ok(response)
                },
            )
            .await
            .unwrap();
            let message = socket.next().await.unwrap().unwrap();
            socket.send(Message::Ping(vec![1].into())).await.unwrap();
            socket.send(message).await.unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Pong(_)
            ));
            let _ = socket.next().await;
        });
        let host = ServerPluginHost::new_with_metadata(String::new(), &HashMap::new());
        let other = ServerPluginHost::new_with_metadata(String::new(), &HashMap::new());
        let result = host
            .execute(
                "http.websocket_open",
                &json!({"url":format!("ws://{address}/"),"proxy":"direct"}),
            )
            .await
            .unwrap();
        let params = json!({"connection_id":result["connection_id"],"text":"你好"});
        assert!(other.execute("http.websocket_send", &params).await.is_err());
        assert_eq!(
            host.execute(
                "http.websocket_receive",
                &json!({"connection_id":result["connection_id"],"timeout_ms":5})
            )
            .await
            .unwrap()["timeout"],
            true
        );
        host.execute("http.websocket_send", &params).await.unwrap();
        assert_eq!(
            host.execute("http.websocket_receive", &params)
                .await
                .unwrap()["text"],
            "你好"
        );
        host.execute("http.websocket_close", &params).await.unwrap();
        assert!(host
            .execute("http.websocket_receive", &params)
            .await
            .is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn websocket_checks_capabilities_and_rejects_insecure_remote_urls() {
        let mut host = ServerPluginHost::new_with_metadata(String::new(), &HashMap::new());
        assert!(host
            .execute("http.websocket_open", &json!({"url":"ws://example.com/"}))
            .await
            .is_err());
        host.policy = Some(policy::Policy::new(
            &json!({"actions":["http.websocket_open"],"network_hosts":["allowed.example"]}),
        ));
        let error = host
            .execute(
                "http.websocket_open",
                &json!({"url":"wss://denied.example/"}),
            )
            .await
            .unwrap_err();
        assert!(error.contains("域名"));
        assert!(host
            .execute(
                "http.websocket_send",
                &json!({"connection_id":"fake","text":"secret"})
            )
            .await
            .unwrap_err()
            .contains("未声明"));
    }
}
