use crate::{server_plugin_host::ServerPluginHost, store::AccountRecord};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{
    tungstenite::{
        handshake::{client::generate_key, derive_accept_key},
        protocol::{Role, WebSocketConfig},
        Message,
    },
    WebSocketStream,
};

pub(crate) type Socket = WebSocketStream<reqwest::Upgraded>;

pub(crate) async fn connect(account: &AccountRecord, descriptor: &Value) -> Result<Socket, String> {
    let mut url = reqwest::Url::parse(descriptor["url"].as_str().ok_or("缺少 WebSocket 地址")?)
        .map_err(|_| "WebSocket 地址无效")?;
    let scheme = match url.scheme() {
        "wss" => "https",
        "ws" => "http",
        _ => return Err("WebSocket 协议无效".into()),
    };
    url.set_scheme(scheme).map_err(|_| "WebSocket 地址无效")?;
    let host = ServerPluginHost::new_for_account(account);
    let key = generate_key();
    let request = host
        .client_for(descriptor)?
        .get(url)
        .version(reqwest::Version::HTTP_11)
        .headers(host.request_headers(descriptor)?)
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", &key);
    let response = tokio::time::timeout(Duration::from_secs(30), request.send())
        .await
        .map_err(|_| "WebSocket 握手超时")?
        .map_err(|_| "WebSocket 连接失败")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("call_not_found".into());
    }
    if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS
        || response
            .headers()
            .get("sec-websocket-accept")
            .and_then(|value| value.to_str().ok())
            != Some(derive_accept_key(key.as_bytes()).as_str())
    {
        return Err("上游拒绝 WebSocket 握手".into());
    }
    let stream = response.upgrade().await.map_err(|_| "WebSocket 升级失败")?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(8 * 1024 * 1024))
        .max_frame_size(Some(8 * 1024 * 1024));
    Ok(WebSocketStream::from_raw_socket(stream, Role::Client, Some(config)).await)
}

pub(crate) struct DriveContext {
    pub store: std::sync::Arc<crate::store::AppStore>,
    pub task_id: String,
    pub account: crate::store::AccountRecord,
    pub connection: Value,
    pub expires_at: i64,
}

pub(crate) async fn drive(
    mut incoming: mpsc::Receiver<Message>,
    outgoing: broadcast::Sender<Message>,
    close_event: Value,
    terminal: Vec<String>,
    context: DriveContext,
) -> Result<(), String> {
    let outcome = loop {
        let remaining = context.expires_at - chrono::Utc::now().timestamp();
        if remaining <= 0 {
            break Err("实时通话已超过最大时长".into());
        }
        let Some(account) = context.store.account_by_id(&context.account.id) else {
            break Err("实时账号已删除".into());
        };
        let account = match context.store.account_with_required_runtime_proxy(account) {
            Ok(account) => account,
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let connected = tokio::time::timeout(
            Duration::from_secs(remaining.min(30) as u64),
            connect(&account, &context.connection),
        )
        .await;
        match connected {
            Ok(Ok(mut socket)) => {
                let ended = drive_connected(
                    &mut socket,
                    &mut incoming,
                    &outgoing,
                    &close_event,
                    &terminal,
                    &context,
                )
                .await;
                let _ = tokio::time::timeout(Duration::from_secs(3), socket.close(None)).await;
                if ended {
                    break Ok(());
                }
            }
            Ok(Err(error)) if error == "call_not_found" => break Ok(()),
            _ => {}
        }
        // 参照观察连接恢复策略：在原会话有效期内重连并保留账号占用，不能重复创建通话。
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    let _ = outgoing.send(Message::Close(None));
    outcome
}

async fn drive_connected(
    socket: &mut Socket,
    incoming: &mut mpsc::Receiver<Message>,
    outgoing: &broadcast::Sender<Message>,
    close_event: &Value,
    terminal: &[String],
    context: &DriveContext,
) -> bool {
    let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
    let mut cancellation = tokio::time::interval(Duration::from_millis(100));
    let mut activity = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _=cancellation.tick()=>{
                let cancelled = context.store.task_by_id(&context.task_id).is_some_and(|task| task.status == "cancelled");
                if cancelled || chrono::Utc::now().timestamp() >= context.expires_at {
                    return send(socket, Message::Text(close_event.to_string().into())).await.is_ok();
                }
            },
            _=heartbeat.tick()=>{
                if activity.elapsed() > Duration::from_secs(60) || send(socket, Message::Ping(Vec::new().into())).await.is_err() { return false; }
            },
            message=incoming.recv()=>match message {
                Some(message)=>{ if send(socket, message).await.is_err() { return false; } },
                None=>return send(socket, Message::Text(close_event.to_string().into())).await.is_ok(),
            },
            frame=socket.next()=> {
                activity = tokio::time::Instant::now();
                match frame {
                    Some(Ok(Message::Ping(bytes)))=>{if send(socket, Message::Pong(bytes)).await.is_err(){return false;}},
                    Some(Ok(Message::Pong(_)))=>{},
                    Some(Ok(Message::Text(text)))=>{
                        let done=serde_json::from_str::<Value>(&text).ok().and_then(|value|value["type"].as_str().map(ToOwned::to_owned)).is_some_and(|kind|terminal.contains(&kind));
                        let _=outgoing.send(Message::Text(text));
                        if done { return true; }
                    },
                    Some(Ok(Message::Close(frame)))=>return frame.as_ref().is_none_or(|frame|frame.code == tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal),
                    None|Some(Err(_))=>return false,
                    Some(Ok(frame))=>{let _=outgoing.send(frame);},
                }
            }
        }
    }
}

async fn send(socket: &mut Socket, message: Message) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message))
        .await
        .map_err(|_| "实时连接写入超时".to_string())?
        .map_err(|_| "实时连接写入失败".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn websocket_upgrade_preserves_auth_and_relays_frames() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut websocket = tokio_tungstenite::accept_hdr_async(
                socket,
                |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                    assert_eq!(request.headers()["authorization"], "Bearer test-only");
                    Ok(response)
                },
            )
            .await
            .unwrap();
            websocket.send(Message::Text("event".into())).await.unwrap();
            assert_eq!(
                websocket.next().await.unwrap().unwrap(),
                Message::Text("reply".into())
            );
        });
        let account = crate::test_fixtures::account_record("a", "test", "Test", "");
        let mut socket=connect(&account,&serde_json::json!({"url":format!("ws://{address}/attach"),"proxy":"direct","headers":{"authorization":"Bearer test-only"}})).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("event".into())
        );
        socket.send(Message::Text("reply".into())).await.unwrap();
        server.await.unwrap();
    }
}
