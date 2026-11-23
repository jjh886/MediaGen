use super::*;
use actix_web::{test, App};
use std::sync::Arc;

fn setup(mode: &str) -> web::Data<ServerState> {
    let original = crate::protocol_api_tests::state();
    let mut state = original.get_ref().clone();
    let root = state.store.path.parent().unwrap();
    state.credential_cipher =
        Arc::new(crate::generation_queue::CredentialCipher::new(root).unwrap());
    let directory = state.plugin_loader.get_plugin_dir("test").unwrap();
    let path = directory.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["models"][0]["capabilities"]["input_modalities"] = json!(["text", "file"]);
    manifest["models"][0]["capabilities"]["attachment_transport"] = json!("workspace");
    manifest["failure_strategy"] = json!([{"contains_any":["quota_exhausted"],"category":"quota_exhausted","retry_action":"failover","account_state":"cooldown","cooldown_seconds":3600,"usage_consumed":false},{"contains_any":["account_model_unavailable"],"category":"account_model_unavailable","retry_action":"failover","usage_consumed":false}]);
    std::fs::write(path, manifest.to_string()).unwrap();
    std::fs::write(directory.join("scripts/text.js"), r#"
function main(ctx) {
 const prior=ctx.request.options?.platform_session;
 if(prior && prior.account !== ctx.account_id) throw new Error('foreign account cursor');
 if(prior && ctx.account_id==='account' && MODE!=='known') {
   host.checkpoint({phase:MODE==='ambiguous'?'accepted':'ready',conversation_id:'original'});
   if(MODE==='permission') throw new Error('account_model_unavailable: model access revoked');
   throw new Error('quota_exhausted: retry after 3600 seconds');
 }
 const text=JSON.stringify(ctx.request.input);
 let answer='FirstAnswer';
 if(ctx.account_id==='other' && !prior) {
   for(const required of ['SystemRules','DeveloperRules','历史标记A','FirstAnswer','继续']) {
     if(!text.includes(required)) throw new Error('missing history: '+required);
   }
   const files=ctx.request.input.flatMap(item=>Array.isArray(item.content)?item.content:[]).filter(p=>p.file_path);
   if(files.length!==1 || host.call('file.read_base64',{path:files[0].file_path}).base64!=='QVRUQUNITUVOVF9NQVJLRVI=') throw new Error('missing attachment');
   answer='MigratedWithAttachment';
 } else if(ctx.account_id==='other') answer='ContinuedOnOther';
 if(MODE==='resume' && ctx.account_id==='other') {
   if(!host.checkpoint().receipt) {
     const path=ctx.profile_dir+'/migration-submissions.json';
     let priorCount=0;try{priorCount=host.call('file.read_json',{path}).count}catch(_){}
     host.call('file.write_json',{path,value:{count:priorCount+1}});
     host.checkpoint({phase:'accepted',receipt:true});
     throw new Error('connection reset');
   }
   answer='RecoveredOnOther';
 }
 const response={id:'resp_'+ctx.request_id,object:'response',model:ctx.request.model,status:'completed',metadata:{platform_session:{account:ctx.account_id,conversation_id:'c_'+ctx.account_id}},output:[{type:'message',role:'assistant',content:[{type:'output_text',text:answer}]}],output_text:answer};
 host.emit({type:'response.created',response:{...response,output:[],status:'in_progress'}});
 host.emit({type:'response.output_text.delta',output_index:0,delta:answer});
 host.emit({type:'response.completed',response});
 return {response};
}
"#.replace("MODE", &serde_json::to_string(mode).unwrap())).unwrap();
    state.plugin_loader = Arc::new(
        mediagen_plugin_runtime::PluginLoader::load_from(directory.parent().unwrap()).unwrap(),
    );
    let mut other = state.store.account_by_id("account").unwrap();
    other.id = "other".into();
    other.profile_dir = state
        .file_root
        .join("other-profile")
        .to_string_lossy()
        .into_owned();
    state.store.insert_account(other);
    web::Data::new(state)
}

async fn file(state: &ServerState) -> String {
    let id = format!("file-{}", uuid::Uuid::new_v4());
    tokio::fs::create_dir_all(&state.file_root).await.unwrap();
    tokio::fs::write(
        crate::file_api::content_path(&state.file_root, &id),
        b"ATTACHMENT_MARKER",
    )
    .await
    .unwrap();
    crate::file_api::write_record(
        &state.file_root,
        &crate::file_api::FileRecord {
            id: id.clone(),
            object: "file".into(),
            filename: "context.txt".into(),
            purpose: "user_data".into(),
            bytes: 17,
            created_at: chrono::Utc::now().timestamp(),
            mime_type: "text/plain".into(),
            owner_hash: crate::access_control::hash_secret("key-a"),
            used_by: vec![],
        },
    )
    .await
    .unwrap();
    id
}
fn initial(file: &str) -> Value {
    json!([
        {"role":"system","content":"SystemRules"},
        {"role":"developer","content":"DeveloperRules"},
        {"role":"user","content":[{"type":"input_text","text":"历史标记A"},{"type":"input_file","file_id":file}]}
    ])
}
async fn call(state: &web::Data<ServerState>, body: Value, endpoint: &str) -> (u16, Value) {
    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri(endpoint)
            .insert_header(("authorization", "Bearer key-a"))
            .insert_header(("x-client-request-id", "migration-regression"))
            .set_json(body)
            .to_request(),
    )
    .await;
    let status = response.status().as_u16();
    (status, test::read_body_json(response).await)
}

#[actix_web::test]
async fn exhausted_session_migrates_full_history_and_attachment_then_reuses_new_cursor_after_restart(
) {
    let state = setup("known");
    let id = file(&state).await;
    let (status, first) = call(
        &state,
        json!({"model":"text-model","input":initial(&id)}),
        "/v1/responses",
    )
    .await;
    assert_eq!(status, 200, "{first}");
    crate::upload_cleanup::sweep(&state).await.unwrap();
    assert!(crate::file_api::content_path(&state.file_root, &id).exists());
    state
        .store
        .mark_account_state_for_seconds("account", "cooldown", 3600);
    let (status, second) = call(
        &state,
        json!({"model":"text-model","previous_response_id":first["id"],"input":"继续"}),
        "/v1/responses",
    )
    .await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(second["output_text"], "MigratedWithAttachment");
    let mut reloaded = state.get_ref().clone();
    reloaded.store = Arc::new(crate::store::AppStore::load(state.store.path.clone()));
    reloaded.credential_cipher = Arc::new(
        crate::generation_queue::CredentialCipher::new(state.store.path.parent().unwrap()).unwrap(),
    );
    let reloaded = web::Data::new(reloaded);
    let (status, third) = call(
        &reloaded,
        json!({"model":"text-model","previous_response_id":second["id"],"input":"再继续"}),
        "/v1/responses",
    )
    .await;
    assert_eq!(status, 200, "{third}");
    assert_eq!(third["output_text"], "ContinuedOnOther");
    let app = test::init_service(
        App::new()
            .app_data(reloaded.clone())
            .configure(crate::configure_routes),
    )
    .await;
    for response in [&first, &second, &third] {
        let result = test::call_service(
            &app,
            test::TestRequest::delete()
                .uri(&format!(
                    "/v1/responses/{}",
                    response["id"].as_str().unwrap()
                ))
                .insert_header(("authorization", "Bearer key-a"))
                .to_request(),
        )
        .await;
        assert_eq!(result.status(), 200);
    }
    crate::upload_cleanup::sweep(&reloaded).await.unwrap();
    assert!(!crate::file_api::content_path(&state.file_root, &id).exists());
}

#[actix_web::test]
async fn quota_rejection_during_chat_migrates_even_when_client_only_sends_new_message() {
    let state = setup("quota");
    let id = file(&state).await;
    // Chat 请求按公共格式传附件，再由协议层统一为 input_file。
    let mut messages = initial(&id);
    messages[2]["content"][0]["type"] = json!("text");
    messages[2]["content"][1] = json!({"type":"file","file":{"file_id":id}});
    let (status, first) = call(
        &state,
        json!({"model":"text-model","messages":messages,"metadata":{"session_id":"window"}}),
        "/v1/chat/completions",
    )
    .await;
    assert_eq!(status, 200, "{first}");
    crate::upload_cleanup::sweep(&state).await.unwrap();
    assert!(crate::file_api::content_path(&state.file_root, &id).exists());
    let (status,next)=call(&state,json!({"model":"text-model","messages":[{"role":"user","content":"继续"}],"metadata":{"session_id":"window"}}),"/v1/chat/completions").await;
    assert_eq!(status, 200, "{next}");
    assert_eq!(
        next["choices"][0]["message"]["content"],
        "MigratedWithAttachment"
    );
}

#[actix_web::test]
async fn session_moves_with_attachments_when_original_account_loses_model_access() {
    let state = setup("permission");
    let id = file(&state).await;
    let (status, first) = call(
        &state,
        json!({"model":"text-model","input":initial(&id)}),
        "/v1/responses",
    )
    .await;
    assert_eq!(status, 200);
    let (status, next) = call(
        &state,
        json!({"model":"text-model","previous_response_id":first["id"],"input":"继续"}),
        "/v1/responses",
    )
    .await;
    assert_eq!(status, 200, "{next}");
    assert_eq!(next["output_text"], "MigratedWithAttachment");
}

#[actix_web::test]
async fn deleted_attachment_is_reported_without_silently_migrating_incomplete_context() {
    let state = setup("known");
    let id = file(&state).await;
    let (_, first) = call(
        &state,
        json!({"model":"text-model","input":initial(&id)}),
        "/v1/responses",
    )
    .await;
    state
        .store
        .mark_account_state_for_seconds("account", "cooldown", 3600);
    tokio::fs::remove_file(crate::file_api::content_path(&state.file_root, &id))
        .await
        .unwrap();
    let (status, error) = call(
        &state,
        json!({"model":"text-model","previous_response_id":first["id"],"input":"继续"}),
        "/v1/responses",
    )
    .await;
    assert!(matches!(status, 400 | 409), "{error}");
    assert!(state
        .store
        .list_tasks(None)
        .iter()
        .all(|task| task.account_id.as_deref() != Some("other")));
}

#[actix_web::test]
async fn accepted_request_with_unknown_result_is_never_submitted_again_on_another_account() {
    let state = setup("ambiguous");
    let id = file(&state).await;
    let (_, first) = call(
        &state,
        json!({"model":"text-model","input":initial(&id)}),
        "/v1/responses",
    )
    .await;
    let (status, _) = call(
        &state,
        json!({"model":"text-model","previous_response_id":first["id"],"input":"继续"}),
        "/v1/responses",
    )
    .await;
    assert_eq!(status, 429);
    assert!(state
        .store
        .list_tasks(None)
        .iter()
        .all(|task| task.account_id.as_deref() != Some("other")));
}

#[actix_web::test]
async fn interrupted_migration_recovers_new_account_without_reusing_old_cursor_or_resubmitting() {
    let state = setup("resume");
    let id = file(&state).await;
    let (_, first) = call(
        &state,
        json!({"model":"text-model","input":initial(&id)}),
        "/v1/responses",
    )
    .await;
    state
        .store
        .mark_account_state_for_seconds("account", "cooldown", 3600);
    let body = json!({"model":"text-model","previous_response_id":first["id"],"input":"继续"});
    let (status, _) = call(&state, body.clone(), "/v1/responses").await;
    assert_eq!(status, 502);
    let mut reloaded = state.get_ref().clone();
    reloaded.store = Arc::new(crate::store::AppStore::load(state.store.path.clone()));
    reloaded.credential_cipher = Arc::new(
        crate::generation_queue::CredentialCipher::new(state.store.path.parent().unwrap()).unwrap(),
    );
    let reloaded = web::Data::new(reloaded);
    let (status, result) = call(&reloaded, body, "/v1/responses").await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["output_text"], "RecoveredOnOther");
    let account = reloaded.store.account_by_id("other").unwrap();
    let submissions: Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(&account.profile_dir).join("migration-submissions.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(submissions["count"], 1);
}
