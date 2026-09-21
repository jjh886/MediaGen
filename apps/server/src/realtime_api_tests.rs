use super::*;
#[actix_web::test]
async fn sessions_are_owned_and_hangup_forwards_declared_event() {
    let root = std::env::temp_dir().join(format!("session-owner-{}", uuid::Uuid::new_v4()));
    let state = web::Data::new(crate::test_fixtures::server_state(
        root,
        mediagen_plugin_runtime::PluginLoader::empty(),
    ));
    state.store.update_settings(crate::store::SettingsUpdate {
        api_keys: Some("a\nb".into()),
        ..Default::default()
    });
    let (incoming, mut receiver) = mpsc::channel(1);
    let (outgoing, _) = broadcast::channel(1);
    state.realtime.0.lock().unwrap().insert(
        "call_a".into(),
        Session {
            owner: crate::access_control::hash_secret("a"),
            incoming,
            outgoing,
            controller: Arc::new(AtomicBool::new(false)),
            close_event: json!({"type":"close"}),
        },
    );
    let app =
        actix_web::test::init_service(actix_web::App::new().app_data(state).configure(configure))
            .await;
    for (key, status) in [("b", 404), ("a", 200)] {
        let response = actix_web::test::call_service(
            &app,
            actix_web::test::TestRequest::post()
                .uri("/v1/live/sessions/call_a/hangup")
                .insert_header(("authorization", format!("Bearer {key}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status().as_u16(), status);
    }
    assert_eq!(
        receiver.recv().await.unwrap(),
        Message::Text("{\"type\":\"close\"}".into())
    );
}

async fn live_state() -> (web::Data<ServerState>, tokio::task::JoinHandle<()>) {
    use futures_util::SinkExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
        while let Some(Ok(frame)) = socket.next().await {
            match frame {
                Message::Text(text) if text.contains("session.close") => {
                    socket
                        .send(Message::Text(r#"{"type":"session.ended"}"#.into()))
                        .await
                        .unwrap();
                    break;
                }
                Message::Ping(bytes) => {
                    socket.send(Message::Pong(bytes)).await.unwrap();
                }
                _ => {}
            }
        }
    });
    let root = std::env::temp_dir().join(format!("live-contract-{}", uuid::Uuid::new_v4()));
    let dir = root.join("plugins/test/scripts");
    std::fs::create_dir_all(&dir).unwrap();
    let result = json!({"call_id":"call_contract","sdp":"v=0\r\n","connection":{"url":format!("ws://{address}/attach"),"proxy":"direct","headers":{"authorization":"Bearer test-only-secret"}},"close_event":{"type":"session.close"},"terminal_events":["session.ended"]});
    std::fs::write(
        dir.join("live.js"),
        format!("function main(){{return {result}}}"),
    )
    .unwrap();
    std::fs::write(root.join("plugins/test/manifest.json"),json!({"platform_id":"test","display_name":"Test","adapter_mode":"api","models":[{"model_id":"live-test","display_name":"Live","task_kind":"realtime"}],"flows":{"create_realtime":{"steps":[{"action":"plugin.run_js","script_file":"live.js"}]}}}).to_string()).unwrap();
    let loader = mediagen_plugin_runtime::PluginLoader::load_from(&root.join("plugins")).unwrap();
    let state = crate::test_fixtures::server_state(root, loader);
    state.store.update_settings(crate::store::SettingsUpdate {
        api_keys: Some("a\nb".into()),
        ..Default::default()
    });
    state
        .store
        .insert_account(crate::test_fixtures::account_record(
            "live-account",
            "test",
            "Live",
            "",
        ));
    (web::Data::new(state), upstream)
}

#[actix_web::test]
async fn live_creation_encrypts_recovery_state_and_uses_one_request_id() {
    use actix_web::{test as at, App};
    let (state, upstream) = live_state().await;
    let app = at::init_service(
        App::new()
            .app_data(state.clone())
            .wrap(actix_web::middleware::from_fn(
                crate::request_identity::middleware,
            ))
            .configure(crate::configure_routes),
    )
    .await;
    for (body, expected) in [
        (json!({}), 400),
        (
            json!({"session":{"model":"live-test"},"transport":{"type":"websocket"}}),
            400,
        ),
        (
            json!({"session":{"model":"missing"},"transport":{"type":"webrtc","sdp":"v=0"}}),
            404,
        ),
    ] {
        let response = at::call_service(
            &app,
            at::TestRequest::post()
                .uri("/v1/live/sessions")
                .insert_header(("authorization", "Bearer a"))
                .set_json(body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status().as_u16(), expected);
    }
    let response = at::call_service(&app,at::TestRequest::post().uri("/v1/live/sessions").insert_header(("authorization","Bearer a")).set_json(json!({"session":{"model":"live-test"},"transport":{"type":"webrtc","sdp":"v=0\r\n"}})).to_request()).await;
    assert_eq!(response.status(), 201);
    let id = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let task = state.store.task_by_id(&id).unwrap();
    let progress = task.result_data.unwrap();
    assert!(progress.contains("live_ciphertext"));
    assert!(!progress.contains("test-only-secret"));
    let response = at::call_service(
        &app,
        at::TestRequest::post()
            .uri("/v1/live/sessions/call_contract/hangup")
            .insert_header(("authorization", "Bearer b"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 404);
    let response = at::call_service(
        &app,
        at::TestRequest::post()
            .uri("/v1/live/sessions/call_contract/hangup")
            .insert_header(("authorization", "Bearer a"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    tokio::time::timeout(std::time::Duration::from_secs(5), upstream)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if state.store.task_by_id(&id).unwrap().status == "completed" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!state
        .realtime
        .0
        .lock()
        .unwrap()
        .contains_key("call_contract"));
}

#[actix_web::test]
async fn management_cancellation_reaches_live_connection() {
    use actix_web::{test as at, App};
    let (state, upstream) = live_state().await;
    let app = at::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let response = at::call_service(
        &app,
        at::TestRequest::post()
            .uri("/v1/live")
            .insert_header(("authorization", "Bearer a"))
            .set_json(json!({"session":{"model":"live-test"},"sdp":"v=0"}))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let task = state
        .store
        .list_tasks(None)
        .into_iter()
        .find(|task| task.task_kind == "realtime")
        .unwrap();
    state.store.cancel_task(&task.id);
    tokio::time::timeout(std::time::Duration::from_secs(5), upstream)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        state.store.task_by_id(&task.id).unwrap().status,
        "cancelled"
    );
}

#[actix_web::test]
async fn persisted_live_session_restores_without_creating_another_call() {
    let (state, upstream) = live_state().await;
    let account = state.store.account_by_id("live-account").unwrap();
    let manifest = state.plugin_loader.get_plugin("test").unwrap();
    let result = run_plugin(
        &state,
        manifest,
        &account,
        "create_realtime",
        json!({}),
        None,
        "restore-task",
        Value::Null,
    )
    .await
    .unwrap();
    let (_, lease) = crate::protocol_task::reserve(
        &state,
        manifest,
        "live-test",
        "realtime",
        "a",
        "restore-task",
        None,
    )
    .unwrap();
    let record = json!({"owner":crate::access_control::hash_secret("a"),"result":result,"expires_at":chrono::Utc::now().timestamp()+60});
    state.store.save_task_progress(
        "restore-task",
        json!({"live_ciphertext":state.credential_cipher.encrypt(&record.to_string()).unwrap()}),
    );
    let mut restored = state.get_ref().clone();
    let restored_path = state.store.path.with_file_name("restored.json");
    std::fs::copy(&state.store.path, &restored_path).unwrap();
    restored.store = Arc::new(crate::store::AppStore::load(restored_path));
    restored.realtime = Sessions::default();
    let restored = web::Data::new(restored);
    assert_eq!(
        restored.store.task_by_id("restore-task").unwrap().status,
        "processing"
    );
    restored.store.update_account(
        "live-account",
        crate::store::AccountUpdate {
            proxy_id: Some("temporarily-missing".into()),
            ..Default::default()
        },
    );
    recover_sessions(restored.clone());
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert_eq!(
        restored.store.task_by_id("restore-task").unwrap().status,
        "processing"
    );
    assert!(restored
        .store
        .protocol_leases
        .lock()
        .unwrap()
        .contains("restore-task"));
    restored.store.update_account(
        "live-account",
        crate::store::AccountUpdate {
            proxy_id: Some(String::new()),
            ..Default::default()
        },
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let session = restored
                .realtime
                .0
                .lock()
                .unwrap()
                .get("call_contract")
                .cloned();
            if let Some(session) = session {
                session
                    .incoming
                    .send(Message::Text(session.close_event.to_string().into()))
                    .await
                    .unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), upstream)
        .await
        .unwrap()
        .unwrap();
    // 原存储对象仅用于构造重启快照；避免其析构覆盖恢复实例的状态。
    state
        .store
        .complete_task("restore-task", json!({"status":"fixture_finished"}));
    drop(lease);
}
