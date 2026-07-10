use actix_web::{web, HttpRequest, HttpResponse, Responder};
use bytes::Bytes;
use futures_util::{stream, Stream};
use mediagen_browser_gateway::{BrowserGatewayError, BrowserSessionEvent};
use serde::Deserialize;

use crate::browser_session_public::public_browser_event_data;
use crate::browser_sessions::gateway_error_response;
use crate::ServerState;

#[derive(Debug, Deserialize)]
pub struct BrowserSessionEventsQuery {
    #[serde(default)]
    after: u64,
}

pub async fn browser_session_events(
    state: web::Data<ServerState>,
    path: web::Path<String>,
    request: HttpRequest,
    query: web::Query<BrowserSessionEventsQuery>,
) -> impl Responder {
    let session_id = path.into_inner();
    if let Err(err) = state.browser_gateway.get_session(&session_id) {
        return gateway_error_response(err);
    }
    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/event-stream; charset=utf-8"))
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(stream_browser_events(
            state,
            session_id,
            read_event_after(&request, query.after),
        ))
}

fn stream_browser_events(
    state: web::Data<ServerState>,
    session_id: String,
    after: u64,
) -> impl Stream<Item = Result<Bytes, actix_web::Error>> {
    stream::unfold(
        (state, session_id, after),
        |(state, session_id, after)| async move {
            let events = wait_for_browser_events(&state, &session_id, after)
                .await
                .ok()?;
            let next_after = events.last().map(|event| event.sequence).unwrap_or(after);
            let chunk = Bytes::from(format_sse_events(events));
            Some((Ok(chunk), (state, session_id, next_after)))
        },
    )
}

async fn wait_for_browser_events(
    state: &web::Data<ServerState>,
    session_id: &str,
    after: u64,
) -> Result<Vec<BrowserSessionEvent>, BrowserGatewayError> {
    let deadline = actix_web::rt::time::Instant::now() + std::time::Duration::from_secs(25);
    loop {
        let events = state.browser_gateway.events_since(session_id, after)?;
        if !events.is_empty() || actix_web::rt::time::Instant::now() >= deadline {
            return Ok(events);
        }
        actix_web::rt::time::sleep(std::time::Duration::from_millis(45)).await;
    }
}

fn read_event_after(request: &HttpRequest, query_after: u64) -> u64 {
    request
        .headers()
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(query_after)
}

fn format_sse_events(events: Vec<BrowserSessionEvent>) -> String {
    let mut body = String::new();
    for event in events {
        let data = serde_json::to_string(&public_browser_event_data(event.data))
            .unwrap_or_else(|_| "{}".to_string());
        body.push_str(&format!("id: {}\n", event.sequence));
        body.push_str(&format!("event: {}\n", event.event));
        body.push_str("data: ");
        body.push_str(&data);
        body.push_str("\n\n");
    }
    if body.is_empty() {
        body.push_str(": heartbeat\n\n");
    }
    body
}

#[cfg(test)]
mod tests {
    use actix_web::test::TestRequest;
    use chrono::Utc;
    use mediagen_browser_gateway::BrowserSessionEvent;
    use serde_json::json;

    use super::{format_sse_events, read_event_after};

    #[test]
    fn last_event_id_header_wins_over_query_after() {
        let request = TestRequest::default()
            .insert_header(("last-event-id", "12"))
            .to_http_request();
        assert_eq!(read_event_after(&request, 5), 12);
    }

    #[test]
    fn sse_events_hide_proxy_credentials() {
        let body = format_sse_events(vec![BrowserSessionEvent {
            sequence: 1,
            event: "status".to_string(),
            data: json!({
                "id": "session-1",
                "metadata": {
                    "proxy_server": "socks5h://user:secret@proxy.example:1080",
                    "proxy_username": "user",
                    "proxy_password": "secret"
                }
            }),
            created_at: Utc::now(),
        }]);

        assert!(body.contains("socks5h://proxy.example:1080"));
        assert!(!body.contains("secret"));
        assert!(!body.contains("proxy_username"));
        assert!(!body.contains("proxy_password"));
    }
}
