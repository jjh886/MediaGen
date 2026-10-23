use crate::store::AccountRecord;
use std::sync::Arc;

use mediagen_browser_gateway::{BrowserGateway, BrowserGatewayConfig};
use mediagen_plugin_runtime::PluginLoader;

use crate::generation_queue::CredentialCipher;
use crate::image_task_queue::ImageTaskQueue;
use crate::store::AppStore;
use crate::ServerState;

#[macro_export]
macro_rules! init_admin_test_app {
    ($state:expr) => {
        actix_web::test::init_service(
            actix_web::App::new()
                .app_data(actix_web::web::Data::new($state))
                .configure($crate::admin_routes::configure)
                .configure($crate::configure_routes),
        )
        .await
    };
}

pub fn account_record(id: &str, platform: &str, name: &str, api_key: &str) -> AccountRecord {
    AccountRecord {
        policy: Default::default(),
        id: id.to_string(),
        platform: platform.to_string(),
        name: name.to_string(),
        email: String::new(),
        avatar_url: String::new(),
        tier: "free".to_string(),
        login_state: "available".to_string(),
        cooldown_until: None,
        used_today: 0,
        used_today_date: "2026-06-29".to_string(),
        daily_limit: 100,
        used_total: 0,
        total_limit: 0,
        concurrency_limit: 1,
        task_interval_seconds: 0,
        proxy_url: String::new(),
        proxy_id: String::new(),
        browser_os: String::new(),
        browser_version: String::new(),
        api_key: api_key.to_string(),
        profile_dir: std::env::temp_dir()
            .join(format!("mediagen-account-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned(),
        browser_profile_dir: String::new(),
        created_at: "2026-06-29T00:00:00Z".to_string(),
    }
}

pub fn server_state(root: std::path::PathBuf, plugin_loader: PluginLoader) -> ServerState {
    ServerState {
        api_bind: ("0.0.0.0".to_string(), 18080),
        admin: Arc::new(
            crate::admin_users_store::AdminStore::open(&root.join("admin.sqlite"), false).unwrap(),
        ),
        plugin_loader: Arc::new(plugin_loader),
        browser_gateway: Arc::new(BrowserGateway::new(BrowserGatewayConfig {
            profile_root: root.join("profiles"),
            launch_browser: false,
            ..BrowserGatewayConfig::default()
        })),
        store: Arc::new(AppStore::load(root.join("server-state.json"))),
        // 测试若显式开启 OAuth 回调，应使用系统分配端口，避免影响其他本机客户端。
        oauth_logins: crate::oauth_login::OAuthLoginStore::new("127.0.0.1:0".to_string()),
        model_catalog: crate::model_api::Catalog::default(),
        realtime: crate::realtime_api::Sessions::default(),
        file_root: root.join("openai-files"),
        image_task_queue: Arc::new(ImageTaskQueue::default()),
        video_task_queue: Arc::new(ImageTaskQueue::default()),
        credential_cipher: Arc::new(CredentialCipher::ephemeral()),
        generation_queue_enabled: true,
        generation_queue_notify: Arc::new(tokio::sync::Notify::new()),
        account_refresh: Arc::new(crate::account_status_refresh::AccountRefreshRuntime::default()),
        weixin: Arc::new(crate::weixin::WeixinRuntime::offline()),
    }
}

pub(crate) async fn callback_receiver() -> (String, tokio::sync::oneshot::Receiver<String>) {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let url = format!("http://{}/callback", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let body = read_callback_body(&mut stream).await;
        let _ = stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .await;
        let _ = tx.send(body);
    });
    (url, rx)
}

pub(crate) async fn read_callback_json(
    received: tokio::sync::oneshot::Receiver<String>,
) -> serde_json::Value {
    let body = tokio::time::timeout(std::time::Duration::from_secs(5), received)
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&body).unwrap()
}

pub(crate) async fn read_callback_body(stream: &mut tokio::net::TcpStream) -> String {
    use tokio::io::AsyncReadExt;

    let mut buffer = Vec::new();
    let mut header_len = None;
    let mut content_len = 0usize;
    loop {
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk).await.unwrap();
        if read == 0 {
            return String::new();
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_len.is_none() {
            if let Some(index) = header_end(&buffer) {
                header_len = Some(index + 4);
                content_len = content_length(&buffer[..index]);
            }
        }
        if let Some(start) = header_len {
            if buffer.len() >= start + content_len {
                return String::from_utf8_lossy(&buffer[start..start + content_len]).to_string();
            }
        }
    }
}

fn header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(headers: &[u8]) -> usize {
    String::from_utf8_lossy(headers)
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0)
}
