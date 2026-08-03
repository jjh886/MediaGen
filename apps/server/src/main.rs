mod access_control;
mod account_browser_profile;
mod account_login;
mod account_login_check;
mod account_login_credentials;
mod admin_access;
mod admin_auth;
mod admin_routes;
mod browser_session_events;
mod browser_session_public;
mod browser_session_setup;
mod browser_session_stream;
mod browser_sessions;
mod callback_host;
mod failure_strategy;
mod file_api;
mod flow_browser_lifecycle;
mod flow_browser_open;
mod flow_runner;
mod flow_runtime;
mod flow_templates;
mod generation_queue;
mod image_api;
mod image_api_response;
mod image_assets;
mod image_file_refs;
mod image_flow_execution;
mod image_multipart;
mod image_request;
mod image_task_completion;
mod image_task_queue;
mod js_flow_runner;
mod net_client;
mod oauth_login;
mod port_process;
mod routes;
mod server_plugin_host;
mod settings;
mod store;
mod store_accounts;
mod store_entities;
mod store_generation_queue;
mod store_helpers;
mod store_models;
mod store_persistence;
mod store_weixin;
mod store_weixin_query;
mod system_control;
mod value_utils;
mod weixin;

#[cfg(test)]
mod browser_session_route_tests;
#[cfg(test)]
mod file_api_tests;
#[cfg(test)]
mod image_api_tests;
#[cfg(test)]
mod routes_login_tests;
#[cfg(test)]
mod routes_tests;
#[cfg(test)]
mod store_dispatch_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod test_fixtures;

use std::path::PathBuf;
use std::sync::Arc;

use actix_files::Files;
use actix_web::{dev::Service, web, App, HttpServer};
use futures_util::future::{ready, Either, FutureExt};
use generation_queue::CredentialCipher;
use image_task_queue::ImageTaskQueue;
use mediagen_browser_gateway::{BrowserGateway, BrowserGatewayConfig};
use mediagen_plugin_runtime::PluginLoader;
use oauth_login::OAuthLoginStore;
use store::AppStore;
use tokio::sync::Notify;

use routes::{
    accounts, cancel_task, check_accounts_login, delete_account, health, models, plugins, settings,
    setup, tasks, toggle_account, update_account, update_settings,
};

#[derive(Clone)]
pub struct ServerState {
    pub plugin_loader: Arc<PluginLoader>,
    pub browser_gateway: Arc<BrowserGateway>,
    pub store: Arc<AppStore>,
    pub oauth_logins: OAuthLoginStore,
    pub file_root: PathBuf,
    pub image_task_queue: Arc<ImageTaskQueue>,
    pub(crate) credential_cipher: Arc<CredentialCipher>,
    pub(crate) generation_queue_enabled: bool,
    pub(crate) generation_queue_notify: Arc<Notify>,
    pub(crate) weixin: Arc<weixin::WeixinRuntime>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = ServerConfig::from_env();
    ensure_server_dirs(&config)?;
    let state = web::Data::new(build_state(&config));
    generation_queue::start_worker(state.clone());
    weixin::start_workers(state.clone());
    let console_dir = config.console_dir.clone();
    let bind = format!("{}:{}", config.host, config.port);

    println!("[mediagen-server] listening on http://{bind}");
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap_fn(|request, service| {
                if admin_access::request_allowed(&request) {
                    Either::Left(
                        service
                            .call(request)
                            .map(|response| response.map(|value| value.map_into_left_body())),
                    )
                } else {
                    Either::Right(ready(Ok(request.into_response(
                        admin_access::unauthorized_response().map_into_right_body(),
                    ))))
                }
            })
            .configure(admin_auth::configure)
            .configure(admin_routes::configure)
            .configure(configure_routes)
            .service(Files::new("/", &console_dir).index_file("index.html"))
    })
    .bind(bind)?
    .run()
    .await
}

fn ensure_server_dirs(config: &ServerConfig) -> std::io::Result<()> {
    // 清除异常退出时遗留的参考图，正常任务结束时仍会立即清理。
    image_assets::cleanup_stale_temp_files();
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
    let (credential_cipher, generation_queue_enabled) =
        match CredentialCipher::new(&config.data_dir) {
            Ok(cipher) => (cipher, true),
            Err(error) => {
                eprintln!("[mediagen-server] 异步生成队列已禁用: {error}");
                (CredentialCipher::ephemeral(), false)
            }
        };
    ServerState {
        plugin_loader: Arc::new(plugin_loader),
        browser_gateway: Arc::new(BrowserGateway::new(BrowserGatewayConfig {
            profile_root: config.profiles_dir.clone(),
            ..BrowserGatewayConfig::default()
        })),
        store: Arc::new(AppStore::load(config.data_dir.join("mediagen.sqlite"))),
        // 固定回调端口只在插件声明的 OAuth 登录流程期间按需监听。
        oauth_logins: OAuthLoginStore::new(config.oauth_callback_bind.clone()),
        file_root: config.data_dir.join("openai-files"),
        image_task_queue: Arc::new(ImageTaskQueue::default()),
        credential_cipher: Arc::new(credential_cipher),
        generation_queue_enabled,
        generation_queue_notify: Arc::new(Notify::new()),
        weixin: Arc::new(weixin::WeixinRuntime::new().unwrap_or_else(|error| {
            eprintln!("[mediagen-server] 微信功能以离线模式启动: {error}");
            weixin::WeixinRuntime::offline()
        })),
    }
}

fn configure_routes(cfg: &mut web::ServiceConfig) {
    configure_management_routes(cfg);
    configure_openai_routes(cfg);
}

fn configure_management_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health))
        .route("/api/setup", web::post().to(setup))
        .route("/api/plugins", web::get().to(plugins))
        .route("/api/accounts", web::get().to(accounts))
        .route(
            "/api/accounts/check-login",
            web::post().to(check_accounts_login),
        )
        .route("/api/accounts/{id}", web::post().to(update_account))
        .route("/api/accounts/{id}", web::delete().to(delete_account))
        .route("/api/accounts/{id}/toggle", web::post().to(toggle_account))
        .route("/api/tasks", web::get().to(tasks))
        .route("/api/tasks/{id}/cancel", web::post().to(cancel_task))
        .route(
            "/api/generation-queue",
            web::get().to(generation_queue::admin_list_queue),
        )
        .route(
            "/api/generation-queue/actions",
            web::post().to(generation_queue::admin_queue_action),
        )
        .route("/api/settings", web::get().to(settings))
        .route("/api/settings", web::post().to(update_settings))
        .configure(weixin::configure)
        .route(
            "/api/system/info",
            web::get().to(system_control::system_info),
        )
        .route("/api/system/exit", web::post().to(system_control::exit_app))
        .route(
            "/api/oauth/callback",
            web::get().to(oauth_login::oauth_callback),
        )
        .configure(browser_sessions::configure);
}

fn configure_openai_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/v1/models", web::get().to(models))
        .route("/v1/files", web::post().to(file_api::upload_file))
        .route("/v1/files", web::get().to(file_api::list_files))
        .route(
            "/v1/files/{file_id}",
            web::get().to(file_api::retrieve_file),
        )
        .route(
            "/v1/files/{file_id}",
            web::delete().to(file_api::delete_file),
        )
        .route(
            "/v1/files/{file_id}/content",
            web::get().to(file_api::retrieve_file_content),
        )
        .route(
            "/v1/images/generations",
            web::post().to(image_api::generate_image),
        )
        .route("/v1/images/edits", web::post().to(image_api::edit_image))
        .route(
            "/v1/images/variations",
            web::post().to(image_api::variation_image),
        )
        .route(
            "/api/v2/generation-queue",
            web::get().to(generation_queue::list_queue),
        )
        .route(
            "/api/v2/generation-queue/actions",
            web::post().to(generation_queue::queue_action),
        )
        .route(
            "/api/v2/generation-tasks/batch",
            web::post().to(generation_queue::batch_task_details),
        )
        .route(
            "/api/v2/generation-tasks/{task_id}",
            web::get().to(generation_queue::task_detail),
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
    oauth_callback_bind: String,
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
            oauth_callback_bind: std::env::var("MEDIAGEN_OAUTH_CALLBACK_BIND")
                .unwrap_or_else(|_| "127.0.0.1:1455".to_string()),
            data_dir,
        }
    }
}

fn env_path(name: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or(fallback)
}
