use crate::store::AccountRecord;
use std::sync::Arc;

use mediagen_browser_gateway::{BrowserGateway, BrowserGatewayConfig};
use mediagen_plugin_runtime::PluginLoader;

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
        id: id.to_string(),
        platform: platform.to_string(),
        name: name.to_string(),
        email: String::new(),
        avatar_url: String::new(),
        tier: "free".to_string(),
        login_state: "available".to_string(),
        cooldown_until: None,
        used_today: 0,
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
        profile_dir: String::new(),
        created_at: "2026-06-29T00:00:00Z".to_string(),
    }
}

pub fn server_state(root: std::path::PathBuf, plugin_loader: PluginLoader) -> ServerState {
    ServerState {
        plugin_loader: Arc::new(plugin_loader),
        browser_gateway: Arc::new(BrowserGateway::new(BrowserGatewayConfig {
            profile_root: root.join("profiles"),
            launch_browser: false,
            ..BrowserGatewayConfig::default()
        })),
        store: Arc::new(AppStore::load(root.join("server-state.json"))),
        oauth_logins: Default::default(),
        file_root: root.join("openai-files"),
        image_task_queue: Arc::new(ImageTaskQueue::default()),
    }
}
