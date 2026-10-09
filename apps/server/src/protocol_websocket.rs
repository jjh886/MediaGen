use crate::ServerState;
use actix_web::{body::MessageBody, web, HttpMessage, HttpRequest, HttpResponse};
use futures_util::{future::poll_fn, StreamExt};
use serde_json::{json, Value};

pub(crate) struct LocalContinuation(pub String);

pub(crate) async fn responses(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return crate::protocol_api::error(401, "invalid_api_key", "API Key 无效");
    }
    let (response, mut session, messages) = match actix_ws::handle(&request, payload) {
        Ok(value) => value,
        Err(_) => {
            return crate::protocol_api::error(400, "invalid_websocket", "WebSocket 握手无效")
        }
    };
    let mut messages = messages
        .max_frame_size(8 * 1024 * 1024)
        .aggregate_continuations()
        .max_continuation_size(8 * 1024 * 1024);
    actix_web::rt::spawn(async move {
        request
            .extensions_mut()
            .insert(crate::conversation_affinity::ConnectionId(
                uuid::Uuid::new_v4().to_string(),
            ));
        let mut previous: Option<(Value, Value)> = None;
        let mut last_model = String::new();
        while let Some(message) = messages.next().await {
            let mut input = match message {
                Ok(actix_ws::AggregatedMessage::Text(text)) if text.len() <= 8 * 1024 * 1024 => {
                    match serde_json::from_str::<Value>(&text) {
                        Ok(value) => value,
                        Err(_) => {
                            let _ =
                                send_error(&mut session, "invalid_json", "消息不是有效 JSON").await;
                            continue;
                        }
                    }
                }
                Ok(actix_ws::AggregatedMessage::Ping(bytes)) => {
                    if session.pong(&bytes).await.is_err() {
                        break;
                    }
                    continue;
                }
                Ok(actix_ws::AggregatedMessage::Pong(_)) => continue,
                _ => break,
            };
            if input["type"] != "response.create" {
                let _ = send_error(
                    &mut session,
                    "invalid_event",
                    "当前接口只接受 response.create",
                )
                .await;
                continue;
            }
            input.as_object_mut().unwrap().remove("type");
            if input.get("model").is_none() {
                input["model"] = json!(last_model);
            }
            if let Some(model) = input["model"].as_str() {
                last_model = model.into();
            }
            request.extensions_mut().remove::<LocalContinuation>();
            if let Some((prior_input, prior_response)) = &previous {
                if input["previous_response_id"]
                    .as_str()
                    .is_some_and(|id| Some(id) == prior_response["id"].as_str())
                {
                    if input["model"] != prior_input["model"] {
                        let _ = send_error(
                            &mut session,
                            "invalid_previous_response",
                            "续接必须使用同一模型",
                        )
                        .await;
                        continue;
                    }
                    let mut items = crate::protocol_storage::input_items(&prior_input["input"]);
                    items.extend(crate::protocol_output::continuation(prior_response));
                    items.extend(crate::protocol_storage::input_items(&input["input"]));
                    input["input"] = json!(items);
                    request.extensions_mut().insert(LocalContinuation(
                        input["previous_response_id"].as_str().unwrap().to_string(),
                    ));
                }
            }
            input["stream"] = json!(true);
            if input.get("store").is_none() {
                input["store"] = json!(false);
            }
            crate::request_identity::new_turn(&request);
            let dispatch = crate::protocol_api::dispatch(
                state.clone(),
                request.clone(),
                web::Bytes::from(input.to_string()),
                "responses",
            );
            tokio::pin!(dispatch);
            let response = loop {
                tokio::select! {
                    response = &mut dispatch => break response,
                    message = messages.next() => match message {
                        Some(Ok(actix_ws::AggregatedMessage::Ping(bytes))) => { if session.pong(&bytes).await.is_err() { return; } },
                        Some(Ok(actix_ws::AggregatedMessage::Pong(_))) => {},
                        Some(Ok(actix_ws::AggregatedMessage::Text(_))) => { if send_error(&mut session, "response_in_progress", "请等待当前响应完成").await.is_err() { return; } },
                        _ => { let _ = session.close(None).await; return; },
                    }
                }
            };
            let success = response.status().is_success();
            let mut body = response.into_body();
            let mut decoder = crate::sse_decoder::Decoder::new(8 * 1024 * 1024);
            let mut error_body = Vec::new();
            loop {
                tokio::select! {
                    message=messages.next()=>match message{
                        Some(Ok(actix_ws::AggregatedMessage::Ping(bytes)))=>{if session.pong(&bytes).await.is_err(){return;}},
                        Some(Ok(actix_ws::AggregatedMessage::Pong(_)))=>{},
                        Some(Ok(actix_ws::AggregatedMessage::Text(_)))=>{if send_error(&mut session,"response_in_progress","请等待当前响应完成").await.is_err(){return;}},
                        _=>{let _=session.close(None).await;return;},
                    },
                    chunk=poll_fn(|cx|std::pin::Pin::new(&mut body).poll_next(cx))=>{
                        let bytes=match chunk{Some(Ok(bytes))=>bytes,Some(Err(_))=>{let _=send_error(&mut session,"upstream_error","事件流中断").await;break;},None=>break};
                        if !success{if error_body.len()+bytes.len()>65536{break;}error_body.extend_from_slice(&bytes);continue;}
                        let blocks=match decoder.push(&bytes){Ok(blocks)=>blocks,Err(_)=>return};
                        for block in blocks{
                            let text=String::from_utf8_lossy(&block);
                            for line in text.lines().filter_map(|line|line.strip_prefix("data: ")){
                                let Ok(event)=serde_json::from_str::<Value>(line)else{continue};
                                if matches!(event["type"].as_str(),Some("response.completed"|"response.incomplete")){previous=Some((input.clone(),event["response"].clone()));}
                                if session.text(event.to_string()).await.is_err(){return;}
                            }
                        }
                    }
                }
            }
            if !success {
                let body = serde_json::from_slice::<Value>(&error_body)
                    .unwrap_or(json!({"error":{"code":"request_failed","message":"请求失败"}}));
                if session
                    .text(json!({"type":"error","error":body["error"]}).to_string())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
        let _ = session.close(None).await;
    });
    response
}

async fn send_error(
    session: &mut actix_ws::Session,
    code: &str,
    message: &str,
) -> Result<(), actix_ws::Closed> {
    session.text(json!({"type":"error","error":{"type":"invalid_request_error","code":code,"message":message,"param":null}}).to_string()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(text: &str) -> web::Bytes {
        let mut bytes = vec![0x81];
        if text.len() < 126 {
            bytes.push(0x80 | text.len() as u8);
        } else {
            bytes.push(0x80 | 126);
            bytes.extend_from_slice(&(text.len() as u16).to_be_bytes());
        }
        let mask = [1, 2, 3, 4];
        bytes.extend_from_slice(&mask);
        bytes.extend(text.bytes().enumerate().map(|(i, byte)| byte ^ mask[i % 4]));
        bytes.into()
    }
    fn events(bytes: &[u8]) -> Vec<Value> {
        let mut cursor = 0;
        let mut result = Vec::new();
        while cursor + 2 <= bytes.len() {
            let opcode = bytes[cursor] & 15;
            let mut size = (bytes[cursor + 1] & 127) as usize;
            cursor += 2;
            if size == 126 {
                size = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
                cursor += 2;
            }
            if cursor + size > bytes.len() {
                break;
            }
            if opcode == 1 {
                if let Ok(value) = serde_json::from_slice(&bytes[cursor..cursor + size]) {
                    result.push(value);
                }
            }
            cursor += size;
        }
        result
    }
    async fn next_response(body: &mut actix_web::body::BoxBody) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let bytes = poll_fn(|cx| std::pin::Pin::new(&mut *body).poll_next(cx))
                    .await
                    .unwrap()
                    .unwrap();
                for event in events(&bytes) {
                    if event["type"] == "response.completed" {
                        return event["response"].clone();
                    }
                }
            }
        })
        .await
        .unwrap()
    }
    #[actix_web::test]
    async fn websocket_continuation_preserves_lineage_and_disconnect_releases_execution() {
        use actix_web::FromRequest;
        let state = crate::protocol_api_tests::state();
        let request = actix_web::test::TestRequest::get()
            .uri("/v1/responses")
            .insert_header(("authorization", "Bearer key-a"))
            .insert_header(("upgrade", "websocket"))
            .insert_header(("connection", "upgrade"))
            .insert_header(("sec-websocket-version", "13"))
            .insert_header(("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="))
            .to_http_request();
        let (mut sender, payload) = actix_http::h1::Payload::create(false);
        let mut payload = actix_web::dev::Payload::from(payload);
        let payload = web::Payload::from_request(&request, &mut payload)
            .await
            .unwrap();
        let response = responses(state.clone(), request, payload).await;
        assert_eq!(response.status(), 101);
        let mut body = response.into_body();
        sender.feed_data(frame(
            &json!({"type":"response.create","model":"text-model","input":"one"}).to_string(),
        ));
        let first = next_response(&mut body).await;
        sender.feed_data(frame(
            &json!({"type":"response.create","previous_response_id":first["id"],"input":"two"})
                .to_string(),
        ));
        let second = next_response(&mut body).await;
        assert_eq!(second["previous_response_id"], first["id"]);
        assert_eq!(second["store"], false);
        assert_ne!(second["id"], first["id"]);
        sender.feed_eof();
        drop(body);
    }
}
