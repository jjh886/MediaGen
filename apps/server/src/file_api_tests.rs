use actix_web::{test as actix_test, web, App};
use mediagen_plugin_runtime::PluginLoader;

use crate::store::SettingsUpdate;
use crate::test_fixtures::server_state;
use crate::ServerState;

macro_rules! auth_get {
    ($uri:expr) => {
        actix_test::TestRequest::get()
            .uri($uri)
            .insert_header(("authorization", "Bearer file-key"))
            .to_request()
    };
}

macro_rules! auth_delete {
    ($uri:expr) => {
        actix_test::TestRequest::delete()
            .uri($uri)
            .insert_header(("authorization", "Bearer file-key"))
            .to_request()
    };
}

macro_rules! upload_request {
    () => {
        actix_test::TestRequest::post()
            .uri("/v1/files")
            .insert_header(("authorization", "Bearer file-key"))
            .insert_header(upload_content_type())
            .set_payload(upload_body())
            .to_request()
    };
}

macro_rules! auth_get_with_key {
    ($uri:expr, $key:expr) => {
        actix_test::TestRequest::get()
            .uri($uri)
            .insert_header(("authorization", format!("Bearer {}", $key)))
            .to_request()
    };
}

macro_rules! upload_request_with_key {
    ($key:expr) => {
        actix_test::TestRequest::post()
            .uri("/v1/files")
            .insert_header(("authorization", format!("Bearer {}", $key)))
            .insert_header(upload_content_type())
            .set_payload(upload_body())
            .to_request()
    };
}

#[actix_web::test]
async fn openai_files_routes_store_read_and_delete_file() {
    let state = test_state("openai-files");
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("file-key=default".to_string()),
        ..SettingsUpdate::default()
    });
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(crate::configure_routes),
    )
    .await;
    let uploaded = call_json(&app, upload_request!()).await;
    let file_id = uploaded["id"].as_str().unwrap();

    let files = call_json(&app, auth_get!("/v1/files")).await;
    assert_eq!(files["data"][0]["id"], file_id);

    let content_response =
        call_response(&app, auth_get!(&format!("/v1/files/{file_id}/content"))).await;
    assert_eq!(
        actix_test::read_body(content_response).await.as_ref(),
        b"hello"
    );
    assert!(
        call_response(&app, auth_delete!(&format!("/v1/files/{file_id}")))
            .await
            .status()
            .is_success()
    );
}

#[actix_web::test]
async fn openai_files_are_isolated_by_api_key() {
    let state = test_state("openai-files-isolation");
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("file-key-a=default\nfile-key-b=default".to_string()),
        ..SettingsUpdate::default()
    });
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(crate::configure_routes),
    )
    .await;
    let uploaded = call_json(&app, upload_request_with_key!("file-key-a")).await;
    let file_id = uploaded["id"].as_str().unwrap();

    let owner_files = call_json(&app, auth_get_with_key!("/v1/files", "file-key-a")).await;
    assert_eq!(owner_files["data"][0]["id"], file_id);

    let other_files = call_json(&app, auth_get_with_key!("/v1/files", "file-key-b")).await;
    assert!(other_files["data"].as_array().unwrap().is_empty());
    assert_eq!(
        call_response(
            &app,
            auth_get_with_key!(&format!("/v1/files/{file_id}/content"), "file-key-b")
        )
        .await
        .status(),
        actix_web::http::StatusCode::NOT_FOUND
    );
}

async fn call_json<S, R, B, E>(app: &S, request: R) -> serde_json::Value
where
    S: actix_web::dev::Service<R, Response = actix_web::dev::ServiceResponse<B>, Error = E>,
    B: actix_web::body::MessageBody + 'static,
    E: std::fmt::Debug,
{
    let response = call_response(app, request).await;
    assert!(response.status().is_success());
    serde_json::from_slice(&actix_test::read_body(response).await).unwrap()
}

async fn call_response<S, R, B, E>(app: &S, request: R) -> actix_web::dev::ServiceResponse<B>
where
    S: actix_web::dev::Service<R, Response = actix_web::dev::ServiceResponse<B>, Error = E>,
    B: actix_web::body::MessageBody + 'static,
    E: std::fmt::Debug,
{
    actix_test::call_service(app, request).await
}

fn upload_content_type() -> (&'static str, &'static str) {
    (
        "content-type",
        "multipart/form-data; boundary=mediagen-test-boundary",
    )
}

fn upload_body() -> &'static str {
    "--mediagen-test-boundary\r\n\
Content-Disposition: form-data; name=\"purpose\"\r\n\r\nvision\r\n\
--mediagen-test-boundary\r\n\
Content-Disposition: form-data; name=\"file\"; filename=\"ref.png\"\r\n\
Content-Type: image/png\r\n\r\nhello\r\n\
--mediagen-test-boundary--\r\n"
}

fn test_state(name: &str) -> ServerState {
    let root = std::env::temp_dir().join(format!("mediagen-file-api-test-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    server_state(root, PluginLoader::empty())
}

#[actix_web::test]
async fn file_list_pagination_filters_and_errors_follow_openai_shape() {
    let state = test_state("openai-file-pagination");
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("file-key=default".into()),
        ..SettingsUpdate::default()
    });
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(crate::configure_routes),
    )
    .await;
    for _ in 0..2 {
        let response = actix_test::call_service(&app, upload_request!()).await;
        assert!(response.status().is_success());
    }
    let response = actix_test::call_service(&app, auth_get!("/v1/files?limit=1&order=asc")).await;
    let first: serde_json::Value = actix_test::read_body_json(response).await;
    assert_eq!(first["data"].as_array().unwrap().len(), 1);
    assert_eq!(first["has_more"], true);
    let path = format!(
        "/v1/files?limit=1&order=asc&after={}",
        first["last_id"].as_str().unwrap()
    );
    let response = actix_test::call_service(&app, auth_get!(&path)).await;
    let second: serde_json::Value = actix_test::read_body_json(response).await;
    assert_eq!(second["has_more"], false);
    assert_ne!(first["last_id"], second["last_id"]);
    let response = actix_test::call_service(&app, auth_get!("/v1/files?purpose=batch")).await;
    let empty: serde_json::Value = actix_test::read_body_json(response).await;
    assert_eq!(empty["data"].as_array().unwrap().len(), 0);
    let response = actix_test::call_service(&app, auth_get!("/v1/files?limit=0")).await;
    assert_eq!(response.status(), 400);
    let error: serde_json::Value = actix_test::read_body_json(response).await;
    assert!(error["error"].get("param").is_some());
}

#[actix_web::test]
async fn large_upload_is_streamed_and_owned_file_id_becomes_an_isolated_workspace_file() {
    use serde_json::{json, Value};
    let mut state = test_state("large-workspace-file");
    let plugins = state.file_root.parent().unwrap().join("plugins/stream");
    std::fs::create_dir_all(&plugins).unwrap();
    let caps = json!({"attachment_transport":"workspace","attachment_limits":{"file_bytes":52*1024*1024},"input_modalities":["file"],"file_extensions":["txt"]});
    std::fs::write(plugins.join("manifest.json"),json!({"platform_id":"stream","display_name":"Stream","models":[{"model_id":"stream","display_name":"Stream","task_kind":"text","capabilities":caps}]}).to_string()).unwrap();
    state.plugin_loader = PluginLoader::load_from(plugins.parent().unwrap())
        .unwrap()
        .into();
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("file-key=default\nother-key=default".into()),
        ..Default::default()
    });
    let state = web::Data::new(state);
    let app = actix_test::init_service(
        App::new()
            .app_data(state.clone())
            .configure(crate::configure_routes),
    )
    .await;
    let size = 51 * 1024 * 1024;
    let prefix = "--large\r\nContent-Disposition: form-data; name=\"file\"; filename=\"完整剧本？.txt\"\r\nContent-Type: text/plain\r\n\r\n";
    let mut body = prefix.as_bytes().to_vec();
    body.resize(body.len() + size, b'x');
    body.extend_from_slice(b"\r\n--large\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nuser_data\r\n--large--\r\n");
    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/files")
            .insert_header(("authorization", "Bearer file-key"))
            .insert_header(("content-type", "multipart/form-data; boundary=large"))
            .set_payload(body)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let uploaded: Value = actix_test::read_body_json(response).await;
    assert_eq!(uploaded["bytes"], size);
    assert_eq!(uploaded["purpose"], "user_data");
    let original = json!({"model":"stream","input":[{"role":"user","content":[{"type":"input_file","file_id":uploaded["id"],"filename":"forged.txt","size":1}]}]});
    assert!(
        crate::protocol_files::resolve(&state, "other-key", &mut original.clone(), &caps)
            .await
            .is_err()
    );
    let mut request = original;
    crate::protocol_files::resolve(&state, "file-key", &mut request, &caps)
        .await
        .unwrap();
    let mut smaller_caps = caps.clone();
    smaller_caps["attachment_limits"]["file_bytes"] = json!(size - 1);
    assert!(crate::protocol_attachments::prepare(
        &state,
        "file-key",
        &mut request.clone(),
        &smaller_caps
    )
    .await
    .is_err());
    crate::protocol_attachments::prepare(&state, "file-key", &mut request, &caps)
        .await
        .unwrap();
    assert!(request.to_string().len() < 1024);
    let part = &request["input"][0]["content"][0];
    assert_eq!(part["filename"], "完整剧本？.txt");
    assert_eq!(part["size"], size);
    assert!(part.get("file_data").is_none());
    assert!(crate::protocol_request::validate_request(&request, "responses").is_err());
    let source = std::path::PathBuf::from(part["file_path"].as_str().unwrap());
    let work = tempfile::tempdir().unwrap();
    crate::protocol_files::materialize(&state, &mut request, work.path())
        .await
        .unwrap();
    let target = std::path::PathBuf::from(
        request["input"][0]["content"][0]["file_path"]
            .as_str()
            .unwrap(),
    );
    assert!(target.starts_with(work.path()));
    assert_eq!(std::fs::metadata(&target).unwrap().len(), size as u64);
    std::fs::write(&target, b"changed").unwrap();
    assert_eq!(std::fs::metadata(&source).unwrap().len(), size as u64);
    let mut forged = json!({"model":"stream","input":[{"content":[{"type":"input_file","file_path":source,"size":size}]}]});
    assert!(crate::protocol_request::validate_request(&forged, "responses").is_err());
    forged["input"][0]["content"][0]["file_path"] = json!(target);
    assert!(
        crate::protocol_files::materialize(&state, &mut forged, work.path())
            .await
            .is_err()
    );
    let before = std::fs::read_dir(&state.file_root).unwrap().count();
    let incomplete = "--broken\r\nContent-Disposition: form-data; name=\"file\"; filename=\"broken.txt\"\r\n\r\nunfinished";
    let failed = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/files")
            .insert_header(("authorization", "Bearer file-key"))
            .insert_header(("content-type", "multipart/form-data; boundary=broken"))
            .set_payload(incomplete)
            .to_request(),
    )
    .await;
    assert_eq!(failed.status(), 400);
    assert_eq!(std::fs::read_dir(&state.file_root).unwrap().count(), before);
    let mut too_large = prefix.as_bytes().to_vec();
    too_large.resize(too_large.len() + 52 * 1024 * 1024 + 1, b'x');
    too_large.extend_from_slice(b"\r\n--large--\r\n");
    let rejected = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/v1/files")
            .insert_header(("authorization", "Bearer file-key"))
            .insert_header(("content-type", "multipart/form-data; boundary=large"))
            .set_payload(too_large)
            .to_request(),
    )
    .await;
    assert_eq!(rejected.status(), 413);
    assert_eq!(std::fs::read_dir(&state.file_root).unwrap().count(), before);
}
