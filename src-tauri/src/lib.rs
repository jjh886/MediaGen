pub mod account_manager;
pub mod account_verification;
mod account_verification_policy;
pub mod api_server;
pub mod browser_manager;
pub mod browser_verification;
mod commands;
pub mod engine;
pub mod error;
pub mod failure_strategy;
pub mod log_layer;
pub mod login_checker;
mod login_checker_flow;
pub mod orchestrator;
pub mod plugin_loader;
pub mod plugin_runtime;
pub mod process_ext;
pub mod scheduler;
pub mod storage;
pub mod sync_ext;
pub mod tier_override;
pub mod types;

use std::path::PathBuf;
use std::sync::Arc;

use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

use engine::EngineRegistry;
use plugin_loader::PluginLoader;
use scheduler::{Scheduler, TaskQueue};
use storage::Storage;
use sync_ext::MutexExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub storage: Arc<Mutex<Storage>>,
    pub plugin_loader: Arc<PluginLoader>,
    pub scheduler: Arc<Mutex<Scheduler>>,
    pub image_task_queue: Arc<TaskQueue>,
    pub engine_registry: Arc<EngineRegistry>,
    /// 直接引用 BrowserEngine，方便需要具体类型方法（如 `register_external_browser`、
    /// `shutdown_all_persistent`）的场景无需从 registry 里 downcast。
    pub browser_engine: Arc<engine::browser_engine::BrowserEngine>,
    pub app_data_dir: PathBuf,
}

pub(crate) use commands::resolve_chrome_path;

pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            handle_duplicate_launch(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = init_app_state(app)?;
            start_api_server_thread(state.clone());
            start_periodic_login_checker(state.clone());
            app.manage(state);
            setup_tray_icon(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_plugins,
            commands::get_accounts,
            commands::delete_account,
            commands::update_account,
            commands::toggle_account,
            commands::get_tasks,
            commands::cancel_task,
            commands::get_settings,
            commands::set_setting,
            commands::check_chrome_available,
            commands::check_chrome_version,
            commands::macos_permissions::check_macos_startup_permissions,
            commands::macos_permissions::open_macos_privacy_settings,
            commands::get_cache_size,
            commands::clear_cache,
            commands::account::add_account_start,
            commands::account::relogin_account,
            commands::account::check_accounts_login,
            commands::batch_register,
            commands::get_logs,
            commands::clear_logs,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // App 退出前统一关掉 persistent 模式保留下来的 Chrome 进程，避免残留
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app_handle.try_state::<Arc<AppState>>() {
                    state.browser_engine.shutdown_all_persistent();
                }
            }
        });
}

fn init_tracing() {
    let env_filter = EnvFilter::from_default_env().add_directive("mediagen=debug".parse().unwrap());
    let fmt_layer = tracing_subscriber::fmt::layer();

    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.jjh886.mediagen");
    std::fs::create_dir_all(&app_dir).ok();
    let db_path = app_dir.join("mediagen.db");

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    if let Ok(log_layer) = log_layer::SqliteLogLayer::new(&db_path) {
        registry.with(log_layer).init();
    } else {
        registry.init();
    }
}

fn init_app_state(app: &tauri::App) -> Result<Arc<AppState>, Box<dyn std::error::Error>> {
    let app_dir = app
        .path()
        .app_data_dir()
        .expect("failed to get app data dir");
    std::fs::create_dir_all(&app_dir).ok();

    let db_path = app_dir.join("mediagen.db");
    let storage = Storage::new(&db_path).expect("failed to init database");

    let resource_dir = app.path().resource_dir().ok();
    let plugins_dir = find_plugins_dir(&app_dir, resource_dir.as_deref());
    let plugin_loader = PluginLoader::load_from(&plugins_dir).unwrap_or_else(|_| {
        tracing::warn!(
            "No plugins found at {:?}, starting with empty plugin set",
            plugins_dir
        );
        PluginLoader::empty()
    });
    tracing::info!(
        "Loaded plugins from {:?}: {} platforms",
        plugins_dir,
        plugin_loader.plugins().len()
    );

    let storage = Arc::new(Mutex::new(storage));
    let image_task_queue = {
        let storage_guard = storage.lock_recover();
        let max_concurrent =
            read_usize_setting(&storage_guard, "image_task_concurrency", 64, 64, 256);
        let max_queued =
            read_usize_setting(&storage_guard, "image_task_queue_limit", 512, 0, 10000);
        tracing::info!(max_concurrent, max_queued, "图片生成任务队列已初始化");
        Arc::new(TaskQueue::new(max_concurrent, max_queued))
    };
    let (engine_reg, browser_engine) = build_engine_registry(storage.clone());

    Ok(Arc::new(AppState {
        storage,
        plugin_loader: Arc::new(plugin_loader),
        scheduler: Arc::new(Mutex::new(Scheduler::new())),
        image_task_queue,
        engine_registry: Arc::new(engine_reg),
        browser_engine,
        app_data_dir: app_dir,
    }))
}

fn handle_duplicate_launch(app: &tauri::AppHandle) {
    tracing::info!("检测到重复启动请求，聚焦已有窗口");
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn setup_tray_icon(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("MediaGen Studio")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => app.exit(0),
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            let should_show = match event {
                // macOS: 单击打开
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => true,
                // Windows: 双击打开
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => true,
                _ => false,
            };
            if should_show {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn find_plugins_dir(app_dir: &std::path::Path, resource_dir: Option<&std::path::Path>) -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut candidates = vec![resource_dir.map(|d| d.join("plugins"))];

    #[cfg(debug_assertions)]
    {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        candidates.extend([
            Some(manifest_dir.join("../plugins")),
            Some(cwd.join("../plugins")),
            Some(cwd.join("plugins")),
            Some(cwd.join("../../plugins")),
        ]);
    }

    candidates.extend([
        Some(cwd.join("plugins")),
        Some(cwd.join("../plugins")),
        Some(cwd.join("../../plugins")),
        exe_dir.as_ref().map(|d| d.join("plugins")),
        Some(app_dir.join("plugins")),
    ]);

    candidates
        .iter()
        .filter_map(|d| d.as_ref())
        .find(|d| d.is_dir())
        .cloned()
        .unwrap_or_else(|| app_dir.join("plugins"))
}

fn build_engine_registry(
    storage: Arc<Mutex<Storage>>,
) -> (EngineRegistry, Arc<engine::browser_engine::BrowserEngine>) {
    let browser_engine = Arc::new(engine::browser_engine::BrowserEngine::new());
    let reserved_api_ports = {
        let storage_guard = storage.lock_recover();
        let base_port = read_u16_setting(&storage_guard, "api_port", 18080);
        read_api_ports(&storage_guard, base_port)
    };
    let mut reg = EngineRegistry::new();
    reg.register(Box::new(engine::http_engine::HttpEngine::new()));
    reg.register(Box::new(
        engine::callback_server_engine::CallbackServerEngine::with_reserved_ports(
            reserved_api_ports,
        ),
    ));
    reg.register(Box::new(engine::crypto_engine::CryptoEngine::new()));
    reg.register(Box::new(engine::process_engine::ProcessEngine::new()));
    // 通过 Arc 包装器进入 registry，保持 AppState 可以拿到同一份 BrowserEngine
    reg.register(Box::new(engine::browser_engine::SharedBrowserEngine::from(
        browser_engine.clone(),
    )));
    reg.register(Box::new(engine::file_engine::FileEngine::new()));
    reg.register(Box::new(engine::watermark_engine::WatermarkEngine::new()));
    reg.register(Box::new(engine::account_engine::AccountEngine::new(
        storage,
    )));
    (reg, browser_engine)
}

fn start_api_server_thread(state: Arc<AppState>) {
    let config = {
        let storage = state.storage.lock_recover();
        let bind_host = read_string_setting(&storage, "api_address")
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let base_port = read_u16_setting(&storage, "api_port", 18080);
        let ports = read_api_ports(&storage, base_port);
        let allowed_origins = read_string_list_setting(&storage, "api_allowed_origins")
            .unwrap_or_else(|| vec!["*".to_string()]);
        api_server::ApiServerConfig {
            bind_host,
            ports,
            allowed_origins,
        }
    };
    std::thread::spawn(move || {
        tracing::info!(
            host = %config.bind_host,
            ports = ?config.ports,
            "启动本地 API 服务器"
        );
        let sys = actix_rt::System::new();
        sys.block_on(async move {
            if let Err(e) = api_server::start_api_servers(state, config).await {
                tracing::error!("API 服务器启动失败: {}", e);
            }
        });
    });
}

fn read_api_ports(storage: &Storage, base_port: u16) -> Vec<u16> {
    if let Some(raw) = read_string_setting(storage, "api_ports") {
        let ports = parse_port_list(&raw);
        if !ports.is_empty() {
            return ports;
        }
    }

    if let Some(end_port) = read_optional_u16_setting(storage, "api_end_port") {
        return build_port_range(base_port, end_port, 32);
    }

    let count = read_optional_usize_setting(storage, "api_port_count")
        .unwrap_or(10)
        .clamp(1, 32);
    (0..count)
        .filter_map(|offset| base_port.checked_add(offset as u16))
        .collect()
}

fn read_string_setting(storage: &Storage, key: &str) -> Option<String> {
    storage
        .get_setting(key)
        .unwrap_or(None)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_u16_setting(storage: &Storage, key: &str, default: u16) -> u16 {
    read_optional_u16_setting(storage, key)
        .filter(|port| *port != 0)
        .unwrap_or(default)
}

fn read_optional_u16_setting(storage: &Storage, key: &str) -> Option<u16> {
    read_string_setting(storage, key).and_then(|s| {
        serde_json::from_str::<u16>(&s)
            .ok()
            .or_else(|| s.parse::<u16>().ok())
    })
}

fn read_usize_setting(
    storage: &Storage,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> usize {
    read_optional_usize_setting(storage, key)
        .unwrap_or(default)
        .clamp(min, max)
}

fn read_optional_usize_setting(storage: &Storage, key: &str) -> Option<usize> {
    read_string_setting(storage, key).and_then(|s| {
        serde_json::from_str::<usize>(&s)
            .ok()
            .or_else(|| s.parse::<usize>().ok())
    })
}

fn build_port_range(start_port: u16, end_port: u16, max_ports: usize) -> Vec<u16> {
    let start = start_port.min(end_port);
    let end = start_port.max(end_port);
    (start..=end).take(max_ports).collect()
}

fn read_string_list_setting(storage: &Storage, key: &str) -> Option<Vec<String>> {
    read_string_setting(storage, key).map(|s| parse_string_list(&s))
}

fn parse_string_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let items = if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<String>>(trimmed).unwrap_or_default()
    } else {
        trimmed
            .split(|c: char| c == ',' || c == ';' || c == '\n' || c == '\r')
            .map(|s| s.trim().to_string())
            .collect()
    };
    let mut out = Vec::new();
    for item in items {
        if item.is_empty() || out.iter().any(|x: &String| x.eq_ignore_ascii_case(&item)) {
            continue;
        }
        out.push(item);
    }
    out
}

fn parse_port_list(raw: &str) -> Vec<u16> {
    let trimmed = raw.trim();
    let tokens = if trimmed.starts_with('[') {
        match serde_json::from_str::<Vec<u16>>(trimmed) {
            Ok(ports) => return dedupe_ports(ports),
            Err(_) => Vec::new(),
        }
    } else {
        trimmed
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .map(|s| s.trim().to_string())
            .collect()
    };

    let mut ports = Vec::new();
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        if let Some((start, end)) = token.split_once('-') {
            if let (Ok(start), Ok(end)) = (start.trim().parse::<u16>(), end.trim().parse::<u16>()) {
                for port in start.min(end)..=start.max(end) {
                    ports.push(port);
                }
            }
            continue;
        }
        if let Ok(port) = token.parse::<u16>() {
            ports.push(port);
        }
    }
    dedupe_ports(ports)
}

fn dedupe_ports(ports: Vec<u16>) -> Vec<u16> {
    let mut out = Vec::new();
    for port in ports {
        if port == 0 || out.contains(&port) {
            continue;
        }
        out.push(port);
    }
    out
}

fn start_periodic_login_checker(state: Arc<AppState>) {
    login_checker::spawn_periodic_checker(
        state.storage.clone(),
        state.plugin_loader.clone(),
        state.engine_registry.clone(),
    );
}

#[cfg(test)]
mod plugin_dir_tests {
    use super::*;

    #[test]
    fn finds_resource_plugins_before_cwd_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let app_dir = tmp.path().join("app-data");
        let resource_dir = tmp.path().join("resources");
        let resource_plugins = resource_dir.join("plugins");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::create_dir_all(&resource_plugins).unwrap();

        let found = find_plugins_dir(&app_dir, Some(&resource_dir));

        assert_eq!(found, resource_plugins);
    }
}
