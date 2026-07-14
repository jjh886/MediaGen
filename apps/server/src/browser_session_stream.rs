use std::time::Duration;

use actix_web::{web, HttpRequest, HttpResponse};
use actix_ws::{Message, Session};
use base64::prelude::{Engine as _, BASE64_STANDARD};
use futures_util::StreamExt;
use mediagen_browser_gateway::{BrowserInputRequest, BrowserSessionEvent};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::browser_session_public::public_browser_event_data;
use crate::browser_sessions::gateway_error_response;
use crate::ServerState;

#[derive(Debug, Deserialize)]
struct StreamCommand {
    #[serde(default)]
    sequence: u64,
    #[serde(rename = "type")]
    kind: String,
    input: Option<BrowserInputRequest>,
}

pub async fn browser_session_stream(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, actix_web::Error> {
    let session_id = path.into_inner();
    if let Err(err) = state.browser_gateway.get_session(&session_id) {
        return Ok(gateway_error_response(err));
    }
    let (response, session, messages) = actix_ws::handle(&request, body)?;
    actix_web::rt::spawn(run_stream(state, session_id, session, messages));
    Ok(response)
}

async fn run_stream(
    state: web::Data<ServerState>,
    session_id: String,
    mut session: Session,
    mut messages: actix_ws::MessageStream,
) {
    let mut after = 0;
    let mut ticker = actix_web::rt::time::interval(Duration::from_millis(35));
    loop {
        tokio::select! {
            message = messages.next() => {
                if !handle_message(&state, &session_id, &mut session, message).await { break; }
            }
            _ = ticker.tick() => {
                let Ok(events) = state.browser_gateway.events_since(&session_id, after) else { break };
                if let Some(last) = events.last() { after = last.sequence; }
                if !send_events(&mut session, events).await { break; }
            }
        }
    }
    let _ = session.close(None).await;
}

async fn handle_message(
    state: &web::Data<ServerState>,
    session_id: &str,
    session: &mut Session,
    message: Option<Result<Message, actix_ws::ProtocolError>>,
) -> bool {
    match message {
        Some(Ok(Message::Text(text))) => handle_text(state, session_id, session, &text).await,
        Some(Ok(Message::Ping(bytes))) => session.pong(&bytes).await.is_ok(),
        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => false,
        _ => true,
    }
}

async fn handle_text(
    state: &web::Data<ServerState>,
    session_id: &str,
    session: &mut Session,
    text: &str,
) -> bool {
    let Ok(command) = serde_json::from_str::<StreamCommand>(text) else {
        return send_json(
            session,
            json!({ "type": "error", "error": "无效的浏览器指令" }),
        )
        .await;
    };
    if command.kind != "input" {
        return send_json(
            session,
            json!({ "type": "error", "sequence": command.sequence, "error": "不支持的浏览器指令" }),
        )
        .await;
    }
    let Some(input) = command.input else {
        return send_json(
            session,
            json!({ "type": "error", "sequence": command.sequence, "error": "缺少 input" }),
        )
        .await;
    };
    let result = state
        .browser_gateway
        .dispatch_input(session_id, input)
        .await;
    let payload = match result {
        Ok(data) => json!({ "type": "input_result", "sequence": command.sequence, "data": data }),
        Err(err) => {
            json!({ "type": "error", "sequence": command.sequence, "error": err.to_string() })
        }
    };
    send_json(session, payload).await
}

async fn send_events(session: &mut Session, events: Vec<BrowserSessionEvent>) -> bool {
    for event in events {
        let data = public_browser_event_data(event.data);
        if event.event == "frame" {
            if !send_frame(session, event.sequence, data).await {
                return false;
            }
        } else if !send_json(
            session,
            json!({
                "type": "event", "sequence": event.sequence, "event": event.event, "data": data
            }),
        )
        .await
        {
            return false;
        }
    }
    true
}

async fn send_frame(session: &mut Session, sequence: u64, mut data: Value) -> bool {
    let image = data
        .get_mut("image")
        .map(Value::take)
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    if image.is_empty() {
        return true;
    }
    if !send_json(
        session,
        json!({ "type": "frame", "sequence": sequence, "data": data }),
    )
    .await
    {
        return false;
    }
    let Ok(bytes) = BASE64_STANDARD.decode(image) else {
        return true;
    };
    session.binary(bytes).await.is_ok()
}

async fn send_json(session: &mut Session, value: Value) -> bool {
    session.text(value.to_string()).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::StreamCommand;

    #[test]
    fn stream_command_accepts_ordered_mouse_input() {
        let command: StreamCommand = serde_json::from_str(
            r#"{"type":"input","sequence":7,"input":{"kind":"mouse","event":"move","x":12,"y":8}}"#,
        )
        .unwrap();
        assert_eq!(command.kind, "input");
        assert_eq!(command.sequence, 7);
        assert!(command.input.is_some());
    }
}
