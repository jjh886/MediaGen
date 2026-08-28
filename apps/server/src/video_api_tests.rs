use std::time::Duration;

use actix_web::{http::StatusCode, test};
use mediagen_plugin_runtime::PluginLoader;
use serde_json::{json, Value};

use crate::settings::SettingsUpdate;
use crate::test_fixtures::{account_record, server_state};
use crate::ServerState;

#[actix_web::test]
async fn video_create_retrieve_list_content_and_delete_follow_openai_contract() {
    let (content_url, server) = video_content_server().await;
    let state = video_state("lifecycle", &content_url, 0);
    let app = test::init_service(
        actix_web::App::new()
            .app_data(actix_web::web::Data::new(state))
            .configure(crate::configure_routes),
    )
    .await;

    let video_id = create_lifecycle_video(&app).await;
    assert_completed_video(&app, &video_id).await;
    assert_video_list(&app, &video_id).await;
    assert_video_content(&app, &video_id, server).await;
    assert_video_deleted(&app, &video_id).await;
}

#[actix_web::test]
async fn video_resources_are_isolated_by_api_key() {
    let state = video_state("owner", "https://example.test/video.mp4", 0);
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("video-key\nother-key".to_string()),
        ..SettingsUpdate::default()
    });
    let app = test::init_service(
        actix_web::App::new()
            .app_data(actix_web::web::Data::new(state))
            .configure(crate::configure_routes),
    )
    .await;
    let created = call_json(
        &app,
        test::TestRequest::post()
            .uri("/v1/videos")
            .insert_header(("authorization", "Bearer video-key"))
            .set_json(json!({ "model": "fake-video", "prompt": "move" })),
    )
    .await;
    let video_id = created["id"].as_str().unwrap();

    let response = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/v1/videos/{video_id}"))
            .insert_header(("authorization", "Bearer other-key"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn running_video_can_be_cancelled() {
    let state = video_state("cancel", "https://example.test/video.mp4", 250);
    let app = test::init_service(
        actix_web::App::new()
            .app_data(actix_web::web::Data::new(state))
            .configure(crate::configure_routes),
    )
    .await;
    let created = call_json(
        &app,
        test::TestRequest::post()
            .uri("/v1/videos")
            .insert_header(("authorization", "Bearer video-key"))
            .set_json(json!({ "model": "fake-video", "prompt": "move" })),
    )
    .await;
    let video_id = created["id"].as_str().unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    let cancelled = call_json(
        &app,
        test::TestRequest::post()
            .uri(&format!("/v1/videos/{video_id}/cancel"))
            .insert_header(("authorization", "Bearer video-key")),
    )
    .await;
    assert_eq!(cancelled["status"], "failed");
    assert_eq!(cancelled["error"]["code"], "video_cancelled");
}

fn video_state(label: &str, content_url: &str, delay_ms: u64) -> ServerState {
    let root = std::env::temp_dir().join(format!("mediagen-video-api-{label}"));
    let plugin_root = root.join("plugins");
    let plugin_dir = plugin_root.join("fake-video");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_root.join("plugins.index.json"),
        r#"{"plugins":["fake-video"]}"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("manifest.json"),
        json!({
            "platform_id": "fake-video",
            "display_name": "Fake Video",
            "adapter_mode": "api",
            "models": [{
                "model_id": "fake-video",
                "display_name": "Fake Video",
                "task_kind": "video"
            }],
            "video_capabilities": {
                "supports_generation": true,
                "supports_image_reference": true,
                "max_reference_images": 1,
                "max_duration_seconds": 60
            },
            "flows": {
                "generate_video": {
                    "steps": [{
                        "action": "plugin.run_js",
                        "script": format!(
                            "function main() {{ host.sleep({delay_ms}); return {{ videos: [{{ video_url: '{}' }}] }}; }}",
                            content_url.replace('\'', "")
                        ),
                        "save_as": "result"
                    }]
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let state = server_state(root, PluginLoader::load_from(&plugin_root).unwrap());
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("video-key".to_string()),
        ..SettingsUpdate::default()
    });
    state
        .store
        .insert_account(account_record("video-account", "fake-video", "Video", ""));
    state
}

async fn wait_for_video<S>(app: &S, video_id: &str) -> Value
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
{
    for _ in 0..50 {
        let video = call_json(
            app,
            test::TestRequest::get()
                .uri(&format!("/v1/videos/{video_id}"))
                .insert_header(("authorization", "Bearer video-key")),
        )
        .await;
        if video["status"] == "completed" {
            return video;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("video did not complete");
}

async fn create_lifecycle_video<S>(app: &S) -> String
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
{
    let created = call_json(
        app,
        test::TestRequest::post()
            .uri("/v1/videos")
            .insert_header(("authorization", "Bearer video-key"))
            .set_json(json!({
                "model": "fake-video",
                "prompt": "a cat walking",
                "seconds": "8",
                "size": "1280x720"
            })),
    )
    .await;
    assert_eq!(created["object"], "video");
    assert_eq!(created["status"], "queued");
    created["id"].as_str().unwrap().to_string()
}

async fn assert_completed_video<S>(app: &S, video_id: &str)
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
{
    let completed = wait_for_video(app, video_id).await;
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["seconds"], "8");
    assert_eq!(completed["size"], "1280x720");
}

async fn assert_video_list<S>(app: &S, video_id: &str)
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
{
    let list = call_json(
        app,
        test::TestRequest::get()
            .uri("/v1/videos?limit=1")
            .insert_header(("authorization", "Bearer video-key")),
    )
    .await;
    assert_eq!(list["object"], "list");
    assert_eq!(list["data"][0]["id"], video_id);
}

async fn assert_video_content<S>(app: &S, video_id: &str, server: tokio::task::JoinHandle<()>)
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
{
    let response = test::call_service(
        app,
        test::TestRequest::get()
            .uri(&format!("/v1/videos/{video_id}/content"))
            .insert_header(("authorization", "Bearer video-key"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(test::read_body(response).await.as_ref(), b"fake-mp4");
    server.await.unwrap();
}

async fn assert_video_deleted<S>(app: &S, video_id: &str)
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
{
    let deleted = call_json(
        app,
        test::TestRequest::delete()
            .uri(&format!("/v1/videos/{video_id}"))
            .insert_header(("authorization", "Bearer video-key")),
    )
    .await;
    assert_eq!(deleted["object"], "video.deleted");
    assert_eq!(deleted["deleted"], true);
}

async fn call_json<S>(app: &S, request: test::TestRequest) -> Value
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
{
    let response = test::call_service(app, request.to_request()).await;
    assert!(
        response.status().is_success(),
        "status={}",
        response.status()
    );
    test::read_body_json(response).await
}

async fn video_content_server() -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let url = format!("http://{}/video.mp4", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: 8\r\n\r\nfake-mp4",
            )
            .await
            .unwrap();
    });
    (url, task)
}
