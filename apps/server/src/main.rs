mod access_control;
mod account_login;
mod browser_runtime;
mod browser_session_events;
mod browser_sessions;
mod callback_host;
mod flow_runner;
mod flow_runtime;
mod flow_templates;
mod image_api;
mod image_multipart;
mod image_request;
mod js_flow_runner;
mod net_client;
mod oauth_login;
mod port_process;
mod routes;
mod server_plugin_host;
mod settings;
mod store;
mod system_control;
mod value_utils;

#[cfg(test)]
mod routes_tests;
#[cfg(test)]
mod store_tests;

use std::path::PathBuf;
use std::sync::Arc;

use actix_files::Files;
use actix_web::{web, App, HttpServer};
use mediagen_browser_gateway::{BrowserGateway, BrowserGatewayConfig};
use mediagen_plugin_runtime::PluginLoader;
use oauth_login::OAuthLoginStore;
use store::AppStore;

use routes::{
    account_groups, accounts, api_keys, browser_runtime_install_guide, browser_runtimes,
    cancel_task, check_accounts_login, delete_account, health, install_browser_runtime, logs,
    models, plugins, settings, setup, tasks, toggle_account, update_account, update_settings,
};

#[derive(Clone)]
pub struct ServerState {
    pub plugin_loader: Arc<PluginLoader>,
    pub browser_gateway: Arc<BrowserGateway>,
    pub store: Arc<AppStore>,
    pub oauth_logins: OAuthLoginStore,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = ServerConfig::from_env();
    ensure_server_dirs(&config)?;
    let state = web::Data::new(build_state(&config));
    let console_dir = config.console_dir.clone();
    let bind = format!("{}:{}", config.host, config.port);

    println!("[mediagen-server] listening on http://{bind}");
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .configure(configure_routes)
            .service(Files::new("/", &console_dir).index_file("index.html"))
    })
    .bind(bind)?
    .run()
    .await
}

fn ensure_server_dirs(config: &ServerConfig) -> std::io::Result<()> {
    std::fs::create_dir_all(&config.data_dir)?;
    std::fs::create_dir_all(&config.logs_dir)?;
    std::fs::create_dir_all(&config.profiles_dir)?;
    Ok(())
}

fn build_state(config: &ServerConfig) -> ServerState {
    let plugin_loader = PluginLoader::load_from(&config.plugins_dir).unwrap_or_else(|err| {
        eprintln!("[mediagen-server] 插件加载失败: {err}");
        PluginLoader::empty()
    });
    ServerState {
        plugin_loader: Arc::new(plugin_loader),
        browser_gateway: Arc::new(BrowserGateway::new(BrowserGatewayConfig {
            profile_root: config.profiles_dir.clone(),
            ..BrowserGatewayConfig::default()
        })),
        store: Arc::new(AppStore::load(config.data_dir.join("server-state.json"))),
        oauth_logins: OAuthLoginStore::default(),
    }
}

fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health))
        .route("/api/setup", web::post().to(setup))
        .route("/api/plugins", web::get().to(plugins))
        .route("/api/accounts", web::get().to(accounts))
        .route("/api/account-groups", web::get().to(account_groups))
        .route("/api/api-keys", web::get().to(api_keys))
        .route(
            "/api/accounts/check-login",
            web::post().to(check_accounts_login),
        )
        .route("/api/accounts/{id}", web::post().to(update_account))
        .route("/api/accounts/{id}", web::delete().to(delete_account))
        .route("/api/accounts/{id}/toggle", web::post().to(toggle_account))
        .route("/api/tasks", web::get().to(tasks))
        .route("/api/tasks/{id}/cancel", web::post().to(cancel_task))
        .route("/api/settings", web::get().to(settings))
        .route("/api/settings", web::post().to(update_settings))
        .route(
            "/api/system/info",
            web::get().to(system_control::system_info),
        )
        .route("/api/system/exit", web::post().to(system_control::exit_app))
        .route("/api/logs", web::get().to(logs))
        .route(
            "/api/oauth/callback",
            web::get().to(oauth_login::oauth_callback),
        )
        .route("/api/browser-runtimes", web::get().to(browser_runtimes))
        .route(
            "/api/browser-runtimes/install-guide",
            web::get().to(browser_runtime_install_guide),
        )
        .route(
            "/api/browser-runtimes/install",
            web::post().to(install_browser_runtime),
        )
        .configure(browser_sessions::configure)
        .route("/v1/models", web::get().to(models))
        .route(
            "/v1/images/generations",
            web::post().to(image_api::generate_image),
        )
        .route("/v1/images/edits", web::post().to(image_api::edit_image))
        .route(
            "/v1/images/variations",
            web::post().to(image_api::variation_image),
        );
}

struct ServerConfig {
    host: String,
    port: u16,
    data_dir: PathBuf,
    logs_dir: PathBuf,
    profiles_dir: PathBuf,
    plugins_dir: PathBuf,
    console_dir: PathBuf,
}

impl ServerConfig {
    fn from_env() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let data_dir = env_path("MEDIAGEN_DATA_DIR", root.join("data"));
        Self {
            host: std::env::var("MEDIAGEN_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: std::env::var("MEDIAGEN_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(18080),
            logs_dir: env_path("MEDIAGEN_LOGS_DIR", root.join("logs")),
            profiles_dir: env_path("MEDIAGEN_PROFILES_DIR", data_dir.join("profiles")),
            plugins_dir: env_path("MEDIAGEN_PLUGINS_DIR", root.join("plugins")),
            console_dir: env_path("MEDIAGEN_CONSOLE_DIR", root.join("apps/console/dist")),
            data_dir,
        }
    }
}

fn env_path(name: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or(fallback)
}
