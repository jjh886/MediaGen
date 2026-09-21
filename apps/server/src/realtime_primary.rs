use actix_web::{web, HttpRequest, HttpResponse};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

use crate::{protocol_api::error, ServerState};

#[derive(Deserialize)]
pub(crate) struct Query {
    model: String,
}

/// 主 WebSocket 是完整通话，不能复用断线重连的旁路观察连接。
pub(crate) async fn connect(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let query = match web::Query::<Query>::from_query(request.query_string()) {
        Ok(query) if !query.model.trim().is_empty() => query,
        _ => return error(400, "missing_model", "model 必须是非空字符串"),
    };
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let Some((manifest, model, _)) = crate::model_api::resolve(&state, &query.model, &key) else {
        return error(404, "model_not_found", "实时模型不存在");
    };
    if model.task_kind != mediagen_core::TaskKind::Realtime
        || !model.extra.get("capabilities").is_some_and(|caps| {
            caps["transports"]
                .as_array()
                .is_some_and(|items| items.contains(&json!("websocket")))
        })
        || !manifest.flows.contains_key("connect_realtime")
    {
        return error(400, "unsupported_transport", "该模型不支持实时主 WebSocket");
    }
    let (response, mut session, messages) = match actix_ws::handle(&request, payload) {
        Ok(value) => value,
        Err(_) => return error(400, "invalid_websocket", "WebSocket 握手无效"),
    };
    let id = crate::request_identity::id(&request);
    let (account, lease) = match crate::protocol_task::reserve(
        &state,
        &manifest,
        &model.model_id,
        "realtime",
        &key,
        &id,
        None,
    ) {
        Ok(value) => value,
        Err(_) => return error(429, "account_unavailable", "账号正在使用、达到限额或不可用"),
    };
    let descriptor = match crate::protocol_api::run_plugin(
        &state,
        &manifest,
        &account,
        "connect_realtime",
        json!({"model":model.model_id}),
        None,
        &id,
        Value::Null,
    )
    .await
    {
        Ok(value) => value,
        Err(message) => {
            let (status, code, message) = crate::protocol_api::public_failure(&message);
            state.store.fail_task(&id, message.clone());
            return error(status, code, &message);
        }
    };
    let mut upstream =
        match crate::realtime_transport::connect(&account, &descriptor["connection"]).await {
            Ok(socket) => socket,
            Err(message) => {
                state.store.fail_task(&id, message.clone());
                if message.contains("account_permission_denied") {
                    return error(
                        403,
                        "account_permission_denied",
                        "当前上游账号未获实时语音授权",
                    );
                }
                return error(502, "realtime_upstream_rejected", &message);
            }
        };
    state.store.mark_account_usage_for(&account.id, "realtime");
    let mut messages = messages
        .max_frame_size(8 * 1024 * 1024)
        .aggregate_continuations()
        .max_continuation_size(8 * 1024 * 1024);
    actix_web::rt::spawn(async move {
        let _lease = lease;
        let mut timer = tokio::time::interval(Duration::from_secs(20));
        let mut cancellation = tokio::time::interval(Duration::from_millis(200));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3600);
        let mut last_client = tokio::time::Instant::now();
        let result: Result<(), &str> = loop {
            tokio::select! {
                _=cancellation.tick()=>{
                    if tokio::time::Instant::now()>=deadline || state.store.task_by_id(&id).is_some_and(|t| t.status=="cancelled") { break Ok(()); }
                },
                _=timer.tick()=>{
                    if last_client.elapsed()>Duration::from_secs(90) {break Err("客户端连接超时");}
                    if session.ping(b"").await.is_err() || crate::realtime_transport::send(&mut upstream,Message::Ping(Vec::new().into())).await.is_err(){break Err("实时连接已关闭");}
                },
                message=messages.next()=>{
                    last_client=tokio::time::Instant::now();
                    match message {
                        Some(Ok(actix_ws::AggregatedMessage::Text(text)))=>{
                            let mut event = match serde_json::from_str::<Value>(&text) { Ok(value) if value.is_object()=>value, _=>{let _=session.text(json!({"type":"error","error":{"type":"invalid_request_error","code":"invalid_event","message":"事件必须是 JSON 对象","param":null}}).to_string()).await;continue;} };
                            adapt(&mut event,&descriptor["client_event_adapter"]);
                            if crate::realtime_transport::send(&mut upstream,Message::Text(event.to_string().into())).await.is_err(){break Err("发送实时事件失败");}
                        },
                        Some(Ok(actix_ws::AggregatedMessage::Ping(data)))=>{if session.pong(&data).await.is_err(){break Err("客户端已断开");}},
                        Some(Ok(actix_ws::AggregatedMessage::Pong(_)))=>{},
                        Some(Ok(actix_ws::AggregatedMessage::Close(_)))|None=>break Ok(()),
                        _=>break Err("不支持的实时帧"),
                    }
                },
                event=upstream.next()=>match event {
                    Some(Ok(Message::Text(text)))=>{
                        let text = if let Ok(mut value)=serde_json::from_str::<Value>(&text) {adapt(&mut value,&descriptor["server_event_adapter"]);value.to_string()} else {break Err("上游实时事件格式无效")};
                        if !tokio::time::timeout(Duration::from_secs(30),session.text(text)).await.is_ok_and(|value| value.is_ok()){break Err("客户端读取超时");}
                    },
                    Some(Ok(Message::Ping(data)))=>{if upstream.send(Message::Pong(data)).await.is_err(){break Err("上游实时连接已断开");}},
                    Some(Ok(Message::Pong(_)))=>{},
                    Some(Ok(Message::Close(_)))|None=>break Ok(()),
                    _=>break Err("上游实时连接失败"),
                }
            }
        };
        if let Err(message) = result {
            let event = json!({"type":"error","error":{"type":"server_error","code":"realtime_connection_failed","message":message,"param":null}});
            let _ =
                tokio::time::timeout(Duration::from_secs(3), session.text(event.to_string())).await;
        }
        let _ = tokio::time::timeout(Duration::from_secs(3), upstream.close(None)).await;
        let _ = session.close(None).await;
        if state
            .store
            .task_by_id(&id)
            .is_none_or(|t| t.status != "cancelled")
        {
            match result {
                Ok(()) => {
                    state
                        .store
                        .complete_task(&id, json!({"status":"completed"}));
                }
                Err(message) => {
                    state.store.fail_task(&id, message.into());
                }
            }
        }
    });
    response
}

/// 声明式 JSON Pointer 转换；具体字段差异由插件描述，公共层不判断平台。
fn adapt(event: &mut Value, config: &Value) {
    let kind = event["type"].as_str().unwrap_or("").to_string();
    if let Some(alias) = config["type_aliases"].get(&kind).and_then(Value::as_str) {
        event["type"] = json!(alias);
    }
    let rule = &config["events"][&kind];
    for (from, to) in rule["move"].as_object().into_iter().flatten() {
        if let (Some(value), Some(to)) = (event.pointer(from).cloned(), to.as_str()) {
            if set_pointer(event, to, value) && from != to {
                remove_pointer(event, from);
            }
        }
    }
    for (path, value) in rule["set"].as_object().into_iter().flatten() {
        set_pointer(event, path, value.clone());
    }
    for path in rule["remove"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        remove_pointer(event, path);
    }
}
fn set_pointer(value: &mut Value, path: &str, entry: Value) -> bool {
    let Some(path) = path.strip_prefix('/') else {
        return false;
    };
    let keys = path
        .split('/')
        .map(|key| key.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>();
    let Some((last, parents)) = keys.split_last() else {
        return false;
    };
    let mut node = value;
    for key in parents {
        let Some(object) = node.as_object_mut() else {
            return false;
        };
        node = object.entry(key.clone()).or_insert_with(|| json!({}));
    }
    if let Some(object) = node.as_object_mut() {
        object.insert(last.clone(), entry);
        true
    } else {
        false
    }
}
fn remove_pointer(value: &mut Value, path: &str) {
    if let Some((parent, key)) = path.rsplit_once('/') {
        if let Some(object) = value.pointer_mut(parent).and_then(Value::as_object_mut) {
            object.remove(&key.replace("~1", "/").replace("~0", "~"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    #[actix_web::test]
    async fn websocket_relays_events_and_releases_account_without_recreating_upstream() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = listener.local_addr().unwrap();
        let upstream = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            while let Some(Ok(frame)) = socket.next().await {
                match frame {
                    Message::Text(text) => {
                        let event: Value = serde_json::from_str(&text).unwrap();
                        assert_eq!(event["session"]["voice"], "voice");
                        assert!(event["session"].get("type").is_none());
                        socket
                            .send(Message::Text(
                                json!({"type":"response.audio.delta","delta":"YXVkaW8="})
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                    }
                    Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
        let root = std::env::temp_dir().join(format!("primary-ws-{}", uuid::Uuid::new_v4()));
        let dir = root.join("plugins/test/scripts");
        std::fs::create_dir_all(&dir).unwrap();
        let descriptor = json!({"connection":{"url":format!("ws://{upstream_address}"),"proxy":"direct"},"client_event_adapter":{"events":{"session.update":{"move":{"/session/audio/output/voice":"/session/voice"},"remove":["/session/type"]}}},"server_event_adapter":{"type_aliases":{"response.audio.delta":"response.output_audio.delta"}}});
        std::fs::write(
            dir.join("connect.js"),
            format!("function main(){{return {descriptor}}}"),
        )
        .unwrap();
        std::fs::write(root.join("plugins/test/manifest.json"),json!({"platform_id":"test","display_name":"Test","adapter_mode":"api","models":[{"model_id":"voice-test","display_name":"Voice","task_kind":"realtime","capabilities":{"transports":["websocket"]}}],"flows":{"connect_realtime":{"steps":[{"action":"plugin.run_js","script_file":"connect.js"}]}}}).to_string()).unwrap();
        let loader =
            mediagen_plugin_runtime::PluginLoader::load_from(&root.join("plugins")).unwrap();
        let state = web::Data::new(crate::test_fixtures::server_state(root.clone(), loader));
        state.store.update_settings(crate::store::SettingsUpdate {
            api_keys: Some("test-key".into()),
            ..Default::default()
        });
        state
            .store
            .insert_account(crate::test_fixtures::account_record(
                "account", "test", "Account", "",
            ));
        let app = actix_web::test::init_service(
            actix_web::App::new()
                .app_data(state.clone())
                .route("/v1/realtime", web::get().to(connect)),
        )
        .await;
        for (path, auth, status) in [
            ("/v1/realtime", "bad", 401),
            ("/v1/realtime", "test-key", 400),
            ("/v1/realtime?model=missing", "test-key", 404),
            ("/v1/realtime?model=voice-test", "test-key", 400),
        ] {
            let response = actix_web::test::call_service(
                &app,
                actix_web::test::TestRequest::get()
                    .uri(path)
                    .insert_header(("authorization", format!("Bearer {auth}")))
                    .to_request(),
            )
            .await;
            assert_eq!(response.status().as_u16(), status);
            let body: Value = actix_web::test::read_body_json(response).await;
            assert!(body["error"]["code"].is_string());
        }
        let clone = state.clone();
        let server = actix_web::HttpServer::new(move || {
            actix_web::App::new()
                .app_data(clone.clone())
                .route("/v1/realtime", web::get().to(connect))
        })
        .workers(1)
        .bind(("127.0.0.1", 0))
        .unwrap();
        let address = server.addrs()[0];
        let server = server.run();
        let handle = server.handle();
        actix_web::rt::spawn(server);
        let mut request = format!("ws://{address}/v1/realtime?model=voice-test")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("authorization", "Bearer test-key".parse().unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        socket
            .send(Message::Text("invalid json".into()))
            .await
            .unwrap();
        socket.send(Message::Text(json!({"type":"session.update","session":{"type":"realtime","audio":{"output":{"voice":"voice"}}}}).to_string().into())).await.unwrap();
        let mut invalid = false;
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(Ok(frame)) = socket.next().await {
                if let Message::Text(text) = frame {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    if event["type"] == "error" {
                        invalid = true;
                    }
                    if event["type"] == "response.output_audio.delta" {
                        assert_eq!(event["delta"], "YXVkaW8=");
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert!(invalid);
        socket.close(None).await.unwrap();
        let _ = socket.next().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while state
                .store
                .list_tasks(None)
                .iter()
                .any(|task| task.status == "processing")
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(state
            .store
            .list_tasks(None)
            .iter()
            .all(|task| task.status == "completed"));
        assert_eq!(state.store.account_by_id("account").unwrap().used_total, 1);
        upstream.await.unwrap();
        handle.stop(false).await;
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn event_mapping_preserves_unrelated_audio_and_only_moves_present_fields() {
        let mut event = json!({"type":"session.update","session":{"type":"realtime","audio":{"output":{"voice":"voice","format":{"type":"audio/pcm"}}}}});
        adapt(
            &mut event,
            &json!({"events":{"session.update":{"move":{"/session/audio/output/voice":"/session/voice"},"remove":["/session/type"]}}}),
        );
        assert_eq!(event["session"]["voice"], "voice");
        assert!(event["session"]["audio"]["output"].get("voice").is_none());
        assert_eq!(
            event["session"]["audio"]["output"]["format"]["type"],
            "audio/pcm"
        );
        let mut server =
            json!({"type":"session.created","session":{"voice":"voice","turn_detection":null}});
        adapt(
            &mut server,
            &json!({"events":{"session.created":{"move":{"/session/voice":"/session/audio/output/voice","/session/turn_detection":"/session/audio/input/turn_detection"},"set":{"/session/type":"realtime"}}}}),
        );
        assert_eq!(server["session"]["audio"]["output"]["voice"], "voice");
        assert!(server["session"]["audio"]["input"]["turn_detection"].is_null());
        assert_eq!(server["session"]["type"], "realtime");
    }
}
