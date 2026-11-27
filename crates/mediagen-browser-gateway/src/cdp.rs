use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::{handshake::client::generate_key, Message};

use crate::BrowserGatewayError;

const CDP_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// CDP 的截图和浏览器下载结果可能在一个 JSON 帧内返回。
pub async fn connect_cdp_socket(websocket_url: &str) -> Result<crate::websocket::Socket, String> {
    let mut url = reqwest::Url::parse(websocket_url).map_err(|_| "CDP 地址无效")?;
    let scheme = match url.scheme() {
        "ws" => "http",
        "wss" => "https",
        _ => return Err("CDP WebSocket 协议无效".into()),
    };
    url.set_scheme(scheme)
        .map_err(|_| "CDP WebSocket 协议无效")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "初始化 CDP 客户端失败")?;
    let key = generate_key();
    let response = crate::websocket::upgrade_request(client.get(url), &key)
        .send()
        .await
        .map_err(|err| err.without_url().to_string())?;
    if response.headers().contains_key("sec-websocket-protocol") {
        return Err("CDP 返回了未请求的子协议".into());
    }
    crate::websocket::finish_upgrade(response, &key).await
}

pub(crate) struct CdpClient {
    pub(crate) viewport_lock: Mutex<()>,
    sender: mpsc::UnboundedSender<String>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: AtomicU64,
}

impl CdpClient {
    pub(crate) async fn connect(
        websocket_url: &str,
    ) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Value>), BrowserGatewayError> {
        let socket = connect_cdp_socket(websocket_url)
            .await
            .map_err(BrowserGatewayError::Cdp)?;
        let (mut write, mut read) = socket.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let (event_tx, event_rx) = mpsc::unbounded_channel::<Value>();
        let pending = Arc::new(Mutex::new(HashMap::<u64, oneshot::Sender<Value>>::new()));
        let pending_reader = Arc::clone(&pending);

        tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                if write.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            while let Some(message) = read.next().await {
                let Ok(message) = message else { break };
                let Ok(text) = message.into_text() else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                    if let Some(sender) = pending_reader.lock().await.remove(&id) {
                        let _ = sender.send(value);
                    }
                    continue;
                }

                if value.get("method").is_some() {
                    let _ = event_tx.send(value);
                }
            }
        });

        Ok((
            Arc::new(Self {
                viewport_lock: Mutex::new(()),
                sender: tx,
                pending,
                next_id: AtomicU64::new(1),
            }),
            event_rx,
        ))
    }

    pub(crate) async fn send(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, BrowserGatewayError> {
        self.send_with_timeout(method, params, CDP_COMMAND_TIMEOUT)
            .await
    }

    pub(crate) async fn send_with_timeout(
        &self,
        method: &str,
        params: Value,
        command_timeout: Duration,
    ) -> Result<Value, BrowserGatewayError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let message = json!({
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string();

        if self.sender.send(message).is_err() {
            self.pending.lock().await.remove(&id);
            return Err(BrowserGatewayError::Cdp("CDP websocket 已断开".to_string()));
        }

        let response = match timeout(command_timeout, rx).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => return Err(BrowserGatewayError::Cdp(format!("{method} 没有返回响应"))),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(BrowserGatewayError::Cdp(format!("{method} 请求超时")));
            }
        };
        if let Some(error) = response.get("error") {
            return Err(BrowserGatewayError::Cdp(error.to_string()));
        }
        Ok(response.get("result").cloned().unwrap_or_else(|| json!({})))
    }

    pub(crate) fn send_fire_and_forget(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(), BrowserGatewayError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let message = json!({
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string();

        self.sender
            .send(message)
            .map_err(|_| BrowserGatewayError::Cdp("CDP websocket 已断开".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn media_response_can_exceed_websocket_defaults() {
        let size = (64 << 20) + 1;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let request = socket.next().await.unwrap().unwrap();
            let command: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
            let response = json!({"id":command["id"], "result":{"data":"A".repeat(size)}});
            socket
                .send(Message::Text(response.to_string().into()))
                .await
                .unwrap();
        });
        let (client, _events) = CdpClient::connect(&format!("ws://{address}"))
            .await
            .unwrap();
        let result = client
            .send("Page.captureScreenshot", json!({}))
            .await
            .unwrap();
        assert_eq!(result["data"].as_str().unwrap().len(), size);
        peer.await.unwrap();
    }
}
