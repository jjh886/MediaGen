use crate::ServerState;
use actix_web::{
    body::MessageBody, error::PayloadError, web, FromRequest, HttpMessage, HttpRequest,
    HttpResponse,
};
use futures_util::{future::poll_fn, stream::try_unfold, StreamExt};
use serde_json::{json, Value};

pub(crate) struct LocalContinuation(pub String);

// Actix 会按未收齐的帧头预留内存。只把完整帧交给它，缓冲区按实际到达的字节增长。
fn complete_frames(payload: web::Payload) -> actix_web::dev::Payload {
    let frames = try_unfold(
        (payload, bytes::BytesMut::new()),
        |(mut payload, mut buf)| async move {
            loop {
                if let Some(end) = mediagen_browser_gateway::websocket::frame_length(&buf, true)
                    .map_err(|_| PayloadError::EncodingCorrupted)?
                {
                    if buf.len() >= end {
                        return Ok(Some((buf.split_to(end).freeze(), (payload, buf))));
                    }
                }
                match payload.next().await {
                    Some(chunk) => buf.extend_from_slice(&chunk?),
                    None if buf.is_empty() => return Ok(None),
                    None => return Err(PayloadError::Incomplete(None)),
                }
            }
        },
    );
    actix_web::dev::Payload::from(frames.boxed_local())
}

pub(crate) async fn accept(
    request: &HttpRequest,
    payload: web::Payload,
) -> Result<
    (
        HttpResponse,
        actix_ws::Session,
        actix_ws::AggregatedMessageStream,
    ),
    actix_web::Error,
> {
    let mut frames = complete_frames(payload);
    let payload = web::Payload::from_request(request, &mut frames).await?;
    let (response, session, messages) = actix_ws::handle(request, payload)?;
    let messages = messages
        .max_frame_size(isize::MAX as usize)
        .aggregate_continuations()
        .max_continuation_size(isize::MAX as usize);
    Ok((response, session, messages))
}

pub(crate) async fn responses(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return crate::protocol_api::error(401, "invalid_api_key", "API Key 无效");
    }
    let (response, mut session, mut messages) = match accept(&request, payload).await {
        Ok(value) => value,
        Err(_) => {
            return crate::protocol_api::error(400, "invalid_websocket", "WebSocket 握手无效")
        }
    };
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
                Ok(actix_ws::AggregatedMessage::Text(text)) => {
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
                input.clone(),
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
            let mut decoder = crate::sse_decoder::Decoder::default();
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
                        let blocks=decoder.push(&bytes);
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
    fn frame(text: String) -> web::Bytes {
        use tokio_tungstenite::tungstenite::protocol::frame::{
            coding::{Data, OpCode},
            Frame,
        };
        let mut frame = Frame::message(text, OpCode::Data(Data::Text), true);
        frame.header_mut().mask = Some([1, 2, 3, 4]);
        let mut bytes = Vec::with_capacity(frame.len());
        frame.format(&mut bytes).unwrap();
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
    async fn next_event(body: &mut actix_web::body::BoxBody, kind: &str) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let bytes = poll_fn(|cx| std::pin::Pin::new(&mut *body).poll_next(cx))
                    .await
                    .unwrap()
                    .unwrap();
                for event in events(&bytes) {
                    if event["type"] == kind {
                        return event;
                    }
                    assert_ne!(event["type"], "error", "{event}");
                }
            }
        })
        .await
        .unwrap()
    }
    async fn open_socket(
        state: web::Data<ServerState>,
        mut payload: actix_web::dev::Payload,
    ) -> actix_web::body::BoxBody {
        let request = actix_web::test::TestRequest::get()
            .uri("/v1/responses")
            .insert_header(("authorization", "Bearer key-a"))
            .insert_header(("upgrade", "websocket"))
            .insert_header(("connection", "upgrade"))
            .insert_header(("sec-websocket-version", "13"))
            .insert_header(("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="))
            .to_http_request();
        let payload = web::Payload::from_request(&request, &mut payload)
            .await
            .unwrap();
        let response = responses(state.clone(), request, payload).await;
        assert_eq!(response.status(), 101);
        response.into_body()
    }

    #[actix_web::test]
    async fn websocket_continuation_preserves_lineage_and_disconnect_releases_execution() {
        let (mut sender, payload) = actix_http::h1::Payload::create(false);
        let mut body = open_socket(crate::protocol_api_tests::state(), payload.into()).await;
        sender.feed_data(frame(
            json!({"type":"response.create","model":"text-model","input":"one"}).to_string(),
        ));
        let first = next_event(&mut body, "response.completed").await["response"].clone();
        sender.feed_data(frame(
            json!({"type":"response.create","previous_response_id":first["id"],"input":"two"})
                .to_string(),
        ));
        let second = next_event(&mut body, "response.completed").await["response"].clone();
        assert_eq!(second["previous_response_id"], first["id"]);
        assert_eq!(second["store"], false);
        assert_ne!(second["id"], first["id"]);
        sender.feed_eof();
        drop(body);
    }

    #[actix_web::test]
    async fn single_frame_inline_attachment_reaches_plugin_at_model_limit() {
        let size = 51 * 1024 * 1024;
        let root = tempfile::tempdir().unwrap();
        let mut state = crate::test_fixtures::protocol_state_in(root.path().to_owned(), false)
            .get_ref()
            .clone();
        let dir = state.plugin_loader.get_plugin_dir("test").unwrap();
        let manifest_path = dir.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        let caps = &mut manifest["models"][0]["capabilities"];
        caps["input_modalities"] = json!(["text", "file"]);
        caps["attachment_limits"] = json!({"file_bytes":size});
        caps["attachment_transport"] = json!("workspace");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        let script_path = dir.join("scripts/text.js");
        let script = r#"function main(ctx) {
            const f = ctx.request.input[0].content[0];
            const tail = host.call('file.read_base64', {path:f.file_path, offset:f.size-3, length:3});
            const response = {id: 'resp_'+ctx.request_id, object:'response', model:ctx.request.model,
                status:'completed', output:[], metadata:{filename:f.filename, bytes:f.size, tail:tail.base64}};
            host.emit({type:'response.completed',sequence_number:0,response});
            return {response};
        }"#;
        std::fs::write(script_path, script).unwrap();
        state.plugin_loader =
            mediagen_plugin_runtime::PluginLoader::load_from(dir.parent().unwrap())
                .unwrap()
                .into();
        let (mut sender, payload) = actix_http::h1::Payload::create(false);
        let mut body = open_socket(web::Data::new(state), payload.into()).await;
        let message = |extra: &str| {
            let encoded = "YWJj".repeat(size / 3) + extra;
            frame(json!({"type":"response.create","model":"text-model","input":[{"role":"user","content":[{"type":"input_file","filename":"notes.txt","file_data":encoded}]}]}).to_string())
        };
        let bytes = message("");
        // 把同一个 FIN 文本帧分批交给传输层，覆盖帧头跨网络读取的情况。
        sender.feed_data(bytes.slice(..5));
        tokio::task::yield_now().await;
        sender.feed_data(bytes.slice(5..14));
        tokio::task::yield_now().await;
        sender.feed_data(bytes.slice(14..));
        drop(bytes);
        let completed = next_event(&mut body, "response.completed").await;
        assert_eq!(
            completed["response"]["metadata"],
            json!({"filename":"notes.txt","bytes":size,"tail":"YWJj"})
        );
        sender.feed_data(message("YQ=="));
        let rejected = next_event(&mut body, "error").await;
        assert_eq!(rejected["error"]["code"], "invalid_attachment");
        assert!(rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&size.to_string()));
        sender.feed_eof();
        drop(body);
    }
}
