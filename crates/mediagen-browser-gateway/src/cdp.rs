use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::BrowserGatewayError;

const CDP_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct CdpClient {
    sender: mpsc::UnboundedSender<String>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: AtomicU64,
}

impl CdpClient {
    pub(crate) async fn connect(
        websocket_url: &str,
    ) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Value>), BrowserGatewayError> {
        let (socket, _) = connect_async(websocket_url)
            .await
            .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?;
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

        let response = match timeout(CDP_COMMAND_TIMEOUT, rx).await {
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
