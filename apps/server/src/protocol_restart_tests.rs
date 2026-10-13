use actix_web::{test, web, App};
use serde_json::{json, Value};
use std::sync::Arc;

#[actix_web::test]
async fn interrupted_protocol_reopens_store_and_cipher_then_queries_once_in_both_stream_modes() {
    for stream in [false, true] {
        let initial = crate::protocol_api_tests::state();
        let root = initial.store.path.parent().unwrap().to_owned();
        let mut configured = initial.get_ref().clone();
        configured.credential_cipher =
            Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
        let directory = configured.plugin_loader.get_plugin_dir("test").unwrap();
        std::fs::write(directory.join("scripts/text.js"), r#"function main(ctx) {
            const saved = host.checkpoint() || {};
            const path = ctx.profile_dir + '/submissions.json';
            if (!saved.conversation_id) {
                const old = host.tryCall('file.read_json', {path});
                host.call('file.write_json', {path, value:{count:(old.data?.count || 0)+1}});
                host.checkpoint({phase:'accepted',conversation_id:'original'});
                throw new Error('temporary_unavailable: test interrupted after acceptance');
            }
            if(saved.conversation_id!=='original') throw new Error('wrong conversation');
            const response={id:'resp_original',object:'response',status:'completed',model:ctx.request.model,output:[{id:'msg_a',type:'message',role:'assistant',status:'completed',content:[{type:'output_text',text:'原会话结果',annotations:[]}]}]};
            host.emit({type:'response.created',response});
            host.emit({type:'response.output_text.delta',item_id:'msg_a',delta:'原会话结果'});
            host.emit({type:'response.completed',response});
            return {success:true,response};
        }"#).unwrap();
        let body = json!({"model":"text-model","input":"原请求","stream":stream});
        let mut task_id = String::new();
        for pass in 0..3 {
            // 每次重新加载持久化存储与密钥，旧执行器和流对象不能参与恢复。
            if pass > 0 {
                configured.store =
                    Arc::new(crate::store::AppStore::load(root.join("server-state.json")));
                configured.credential_cipher =
                    Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
            }
            let app = test::init_service(
                App::new()
                    .app_data(web::Data::new(configured.clone()))
                    .configure(crate::configure_routes),
            )
            .await;
            let response = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/v1/responses")
                    .insert_header(("authorization", "Bearer key-a"))
                    .insert_header(("x-client-request-id", "original-client-request"))
                    .set_json(&body)
                    .to_request(),
            )
            .await;
            assert_eq!(
                response.status().as_u16(),
                if pass == 0 { 503 } else { 200 }
            );
            let bytes = test::read_body(response).await;
            if pass > 0 {
                assert!(String::from_utf8_lossy(&bytes).contains("原会话结果"));
            }
            let tasks = configured.store.list_tasks(None);
            assert_eq!(tasks.len(), 1);
            if pass == 0 {
                task_id = tasks[0].id.clone();
            }
            assert_eq!(tasks[0].id, task_id);
            assert_eq!(tasks[0].account_id.as_deref(), Some("account"));
            let profile = configured
                .store
                .account_by_id("account")
                .unwrap()
                .profile_dir;
            let counter: Value = serde_json::from_slice(
                &std::fs::read(std::path::Path::new(&profile).join("submissions.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(counter["count"], 1);
        }
        assert_eq!(
            configured
                .store
                .account_by_id("account")
                .unwrap()
                .policy
                .text_used_total,
            1
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[actix_web::test]
async fn recovery_identity_is_scoped_to_caller_input_and_endpoint() {
    let request = test::TestRequest::post()
        .uri("/v1/responses")
        .insert_header(("x-client-request-id", "same"))
        .to_http_request();
    let original =
        crate::request_identity::recovery_id(&request, "owner-a", &json!({"input":"hello"}));
    assert_eq!(
        original,
        crate::request_identity::recovery_id(&request, "owner-a", &json!({"input":"hello"}))
    );
    assert_ne!(
        original,
        crate::request_identity::recovery_id(&request, "owner-b", &json!({"input":"hello"}))
    );
    assert_ne!(
        original,
        crate::request_identity::recovery_id(&request, "owner-a", &json!({"input":"changed"}))
    );
    let chat = test::TestRequest::post()
        .uri("/v1/chat/completions")
        .insert_header(("x-client-request-id", "same"))
        .to_http_request();
    assert_ne!(
        original,
        crate::request_identity::recovery_id(&chat, "owner-a", &json!({"input":"hello"}))
    );
}

#[actix_web::test]
async fn chat_window_keeps_platform_cursor_across_http_turns_and_server_restart() {
    for (mode, stream) in [
        ("desktop", false),
        ("responses", false),
        ("chat", false),
        ("responses", true),
        ("chat", true),
    ] {
        let original = crate::protocol_api_tests::state();
        let root = original.store.path.parent().unwrap().to_owned();
        let mut configured = original.get_ref().clone();
        configured.credential_cipher =
            Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
        let script = configured
            .plugin_loader
            .get_plugin_dir("test")
            .unwrap()
            .join("scripts/text.js");
        std::fs::write(script,r#"function main(ctx){
      const previous=ctx.request.options?.platform_session;
      const id=previous?.conversation_id || ('session-'+ctx.request.options.session_key);
      const round=(previous?.round || 0)+1;
      const response={id:'resp_'+ctx.request_id,object:'response',status:'completed',model:ctx.request.model,metadata:{platform_session:{conversation_id:id,round}},output:[{id:'answer',type:'message',role:'assistant',status:'completed',content:[{type:'output_text',text:id+'|'+round,annotations:[]}]}]};
      host.emit({type:'response.created',response:{...response,output:[],metadata:{}}});
      host.emit({type:'response.output_text.delta',item_id:'answer',delta:id+'|'+round});
      host.emit({type:'response.completed',response});
      return {response};
    }"#).unwrap();
        let mut conversation_ids = std::collections::HashMap::new();
        for (window, expected) in [("first", 1), ("first", 2), ("second", 1), ("first", 3)] {
            configured.store =
                Arc::new(crate::store::AppStore::load(root.join("server-state.json")));
            configured.credential_cipher =
                Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
            let app = test::init_service(
                App::new()
                    .app_data(web::Data::new(configured.clone()))
                    .configure(crate::configure_routes),
            )
            .await;
            let mut request = json!({"model":"text-model","input":"继续","store":false,"stream":stream,"metadata":{"session_id":window}});
            let endpoint = if mode == "chat" {
                request.as_object_mut().unwrap().remove("input");
                request["messages"] = json!([{"role":"user","content":"继续"}]);
                "/v1/chat/completions"
            } else {
                "/v1/responses"
            };
            if mode == "desktop" {
                request["metadata"] = json!({"duanju_mode":"chat","duanju_thread_id":window});
            }
            let response = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri(endpoint)
                    .insert_header(("authorization", "Bearer key-a"))
                    .set_json(request)
                    .to_request(),
            )
            .await;
            assert_eq!(response.status().as_u16(), 200);
            let bytes = test::read_body(response).await;
            let text = if stream {
                let raw = String::from_utf8(bytes.to_vec()).unwrap();
                assert!(!raw.contains("platform_session"));
                let events = raw
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .filter_map(|data| serde_json::from_str::<Value>(data).ok())
                    .collect::<Vec<_>>();
                if mode == "chat" {
                    assert!(raw.ends_with("data: [DONE]\n\n"));
                    events
                        .iter()
                        .filter_map(|event| event["choices"][0]["delta"]["content"].as_str())
                        .collect::<String>()
                } else {
                    events
                        .iter()
                        .find(|event| event["type"] == "response.completed")
                        .unwrap()["response"]["output"][0]["content"][0]["text"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                }
            } else {
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                assert!(body["metadata"].get("platform_session").is_none());
                if mode == "chat" {
                    body["choices"][0]["message"]["content"].as_str()
                } else {
                    body["output"][0]["content"][0]["text"].as_str()
                }
                .unwrap()
                .to_owned()
            };
            let (id, round) = text.rsplit_once('|').unwrap();
            assert_eq!(round, expected.to_string());
            if let Some(previous) = conversation_ids.get(window) {
                assert_eq!(id, previous)
            } else {
                assert!(!conversation_ids.values().any(|old| old == id));
                conversation_ids.insert(window, id.to_owned());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[actix_web::test]
async fn previous_response_continues_platform_session_after_restart_and_keeps_windows_separate() {
    let original = crate::protocol_api_tests::state();
    let root = original.store.path.parent().unwrap().to_owned();
    let mut configured = original.get_ref().clone();
    let script = configured
        .plugin_loader
        .get_plugin_dir("test")
        .unwrap()
        .join("scripts/text.js");
    std::fs::write(script, r#"function main(ctx){
        const cursor=ctx.request.options.platform_session;
        const id=cursor?.conversation_id || ctx.request.options.session_key;
        const round=(cursor?.round || 0)+1;
        if(round>1 && ctx.request.input.length<3) throw new Error('missing history');
        return {response:{id:'resp_'+ctx.request_id,object:'response',status:'completed',model:ctx.request.model,
            metadata:{platform_session:{conversation_id:id,round}},output:[{type:'message',role:'assistant',status:'completed',content:[{type:'output_text',text:id+'|'+round}]}]}};
    }"#).unwrap();
    let mut previous = Value::Null;
    let mut original_id = String::new();
    for turn in 0..4 {
        configured.store = Arc::new(crate::store::AppStore::load(root.join("server-state.json")));
        configured.credential_cipher =
            Arc::new(crate::generation_queue::CredentialCipher::new(&root).unwrap());
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(configured.clone()))
                .configure(crate::configure_routes),
        )
        .await;
        let mut body =
            json!({"model":"text-model","input":"继续","prompt_cache_key":"shared-cache"});
        if turn == 1 || turn == 3 {
            body["previous_response_id"] = previous.clone();
        }
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .set_json(body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), 200);
        let response: Value = test::read_body_json(response).await;
        assert!(response["metadata"].get("platform_session").is_none());
        let (id, round) = response["output"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .rsplit_once('|')
            .unwrap();
        assert_eq!(round, ["1", "2", "1", "3"][turn]);
        if turn == 0 {
            original_id = id.to_owned();
        } else if turn == 2 {
            assert_ne!(original_id, id);
        } else {
            assert_eq!(original_id, id);
        }
        if turn != 2 {
            previous = response["id"].clone();
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
