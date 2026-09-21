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
