use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::health::{is_current_install, is_healthy, wait_for_health};
use crate::platform::append_startup_log;
use mediagen_core::{bundled_cloakbrowser_candidates, first_existing_browser_binary};

const DEFAULT_PORT: u16 = 18080;
const MAX_PORT: u16 = 18089;

struct LauncherPaths {
    install_root: PathBuf,
    writable_root: PathBuf,
}

pub struct LaunchSession {
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub port: u16,
    pub url: String,
}

pub fn run() -> Result<LaunchSession, String> {
    let paths = resolve_paths()?;
    let server = resolve_server_path(&paths.install_root);
    if !server.exists() {
        return Err(format!("没有找到服务程序：{}", server.display()));
    }

    create_dirs(&paths.writable_root)?;
    let port = select_port(&paths.install_root)?;
    if !is_current_install(port, &paths.install_root) {
        start_server(&paths, &server, port)?;
    }
    wait_for_health(port, Duration::from_secs(45))?;
    Ok(LaunchSession {
        port,
        url: format!("http://127.0.0.1:{port}"),
    })
}

fn resolve_paths() -> Result<LauncherPaths, String> {
    let exe = std::env::current_exe().map_err(|e| format!("读取启动器路径失败: {e}"))?;
    let exe_dir = exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "启动器目录无效".to_string())?;

    let install_root = if is_macos_app_macos_dir(&exe_dir) {
        exe_dir
            .parent()
            .map(|contents| contents.join("Resources").join("MediaGen-local"))
            .ok_or_else(|| "macOS App 资源目录无效".to_string())?
    } else {
        exe_dir
    };

    Ok(LauncherPaths {
        install_root,
        writable_root: resolve_writable_root()?,
    })
}

fn is_macos_app_macos_dir(path: &Path) -> bool {
    cfg!(target_os = "macos")
        && path.file_name().and_then(|name| name.to_str()) == Some("MacOS")
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("Contents")
        && path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::extension)
            .and_then(|name| name.to_str())
            == Some("app")
}

fn resolve_server_path(root: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "mediagen-server.exe"
    } else {
        "mediagen-server"
    };
    root.join("bin").join(name)
}

fn resolve_writable_root() -> Result<PathBuf, String> {
    // macOS 的 DMG 挂载目录是只读的，用户数据必须放到系统分配的可写目录。
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").ok_or_else(|| "无法读取 HOME 目录".to_string())?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("MediaGen"));
    }

    if cfg!(target_os = "windows") {
        let appdata =
            std::env::var_os("APPDATA").ok_or_else(|| "无法读取 APPDATA 目录".to_string())?;
        return Ok(PathBuf::from(appdata).join("MediaGen"));
    }

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("mediagen"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| "无法读取 HOME 目录".to_string())?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("mediagen"))
}

fn create_dirs(root: &Path) -> Result<(), String> {
    for dir in ["data", "logs", "profiles"] {
        std::fs::create_dir_all(root.join(dir))
            .map_err(|e| format!("创建目录失败 {}: {e}", root.join(dir).display()))?;
    }
    Ok(())
}

fn start_server(paths: &LauncherPaths, server: &Path, port: u16) -> Result<(), String> {
    let log_path = paths.writable_root.join("logs").join("startup.log");
    append_startup_log(&log_path, "正在启动 mediagen-server");

    let mut command = Command::new(server);
    command
        .current_dir(&paths.install_root)
        .env("MEDIAGEN_PORT", port.to_string())
        .env("MEDIAGEN_DATA_DIR", paths.writable_root.join("data"))
        .env("MEDIAGEN_LOGS_DIR", paths.writable_root.join("logs"))
        .env(
            "MEDIAGEN_PROFILES_DIR",
            paths.writable_root.join("profiles"),
        )
        .env("MEDIAGEN_PLUGINS_DIR", paths.install_root.join("plugins"))
        .env("MEDIAGEN_CONSOLE_DIR", paths.install_root.join("console"))
        .stdin(Stdio::null());

    if let Some(browser_path) = resolve_bundled_cloakbrowser_path(&paths.install_root) {
        command.env("CLOAK_BROWSER_PATH", browser_path);
    }

    attach_startup_log(&mut command, &log_path)?;

    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动 mediagen-server 失败: {e}"))
}

fn select_port(install_root: &Path) -> Result<u16, String> {
    if let Some(port) = configured_port()? {
        if is_current_install(port, install_root) || !is_port_open(port) {
            return Ok(port);
        }
        if is_healthy(port) {
            return Err(format!(
                "端口 {port} 已有旧版 MediaGen 服务在运行，请先从托盘退出旧服务，或换一个 MEDIAGEN_PORT"
            ));
        }
        return Err(format!(
            "端口 {port} 已被占用，请换一个 MEDIAGEN_PORT 或关闭占用程序"
        ));
    }

    for port in DEFAULT_PORT..=MAX_PORT {
        if is_current_install(port, install_root) || !is_port_open(port) {
            return Ok(port);
        }
    }
    Err(format!(
        "端口 {DEFAULT_PORT}-{MAX_PORT} 都被占用，请先关闭旧的 MediaGen 或其他占用端口的程序"
    ))
}

fn configured_port() -> Result<Option<u16>, String> {
    let Some(value) = std::env::var("MEDIAGEN_PORT")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .map(Some)
        .ok_or_else(|| format!("MEDIAGEN_PORT 无效：{value}"))
}

fn is_port_open(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

fn attach_startup_log(command: &mut Command, log_path: &Path) -> Result<(), String> {
    // 服务端的 stdout/stderr 都写入同一个日志，方便普通用户把报错原样发给我们排查。
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| format!("打开启动日志失败 {}: {e}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .map_err(|e| format!("复制启动日志句柄失败: {e}"))?;
    command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    Ok(())
}

fn resolve_bundled_cloakbrowser_path(root: &Path) -> Option<PathBuf> {
    if std::env::var_os("CLOAK_BROWSER_PATH").is_some() {
        return None;
    }
    first_existing_browser_binary(bundled_cloakbrowser_candidates(root))
}
