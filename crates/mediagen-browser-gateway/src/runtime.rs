use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use mediagen_core::{
    bundled_cloakbrowser_candidates, first_existing_browser_binary, path_or_runtime_candidates,
};
use serde::Deserialize;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Duration};

use crate::cdp::CdpClient;
use crate::fingerprint::BrowserFingerprint;
use crate::proxy;
use crate::{BrowserGatewayConfig, BrowserGatewayError, BrowserSession};

#[path = "runtime_targets.rs"]
mod targets;
pub(crate) use targets::{
    close_page_target, create_page_target, get_best_page_target, get_page_target,
    is_useful_page_url, BrowserTarget,
};

const SAFE_LAUNCH_URL: &str = "about:blank";
const RUNTIME_PORT_FILE: &str = ".mediagen-browser-port";
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct BrowserRuntime {
    bridge: Option<Arc<crate::proxy_bridge::Bridge>>,
    pub debugging_port: u16,
    child: Arc<Mutex<Option<Child>>>,
    port_file: PathBuf,
}

impl BrowserRuntime {
    pub(crate) fn owned(debugging_port: u16, child: Child, profile_dir: &str) -> Self {
        Self {
            debugging_port,
            bridge: None,
            child: Arc::new(Mutex::new(Some(child))),
            port_file: runtime_port_file(profile_dir),
        }
    }

    fn attached(debugging_port: u16, profile_dir: &str) -> Self {
        Self {
            debugging_port,
            bridge: crate::proxy_bridge::existing(profile_dir),
            child: Arc::new(Mutex::new(None)),
            port_file: runtime_port_file(profile_dir),
        }
    }

    pub(crate) async fn stop(&self) {
        let mut child = self.child.lock().await;
        // 只有当前服务亲自启动的进程才能关闭；跨重启接管的进程保留端口登记，
        // 后续任务仍可继续接管，不会再次因为资料目录锁报 exit status 21。
        if let Some(mut child) = child.take() {
            // Browser.close 会让 Chromium 正常刷新 Cookie、Local Storage 等 profile 数据。
            // 若浏览器无响应，再退回 kill，保证服务不会永久卡在清理阶段。
            let _ = request_browser_close(self.debugging_port).await;
            let exited = matches!(
                timeout(GRACEFUL_SHUTDOWN_TIMEOUT, child.wait()).await,
                Ok(Ok(_))
            );
            if !exited {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
            let _ = tokio::fs::remove_file(&self.port_file).await;
        }
    }
}

#[derive(Deserialize)]
struct BrowserVersion {
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket_url: String,
}

async fn request_browser_close(debugging_port: u16) -> Result<(), BrowserGatewayError> {
    let version = reqwest::get(format!("http://127.0.0.1:{debugging_port}/json/version"))
        .await
        .map_err(|error| BrowserGatewayError::Cdp(error.to_string()))?
        .error_for_status()
        .map_err(|error| BrowserGatewayError::Cdp(error.to_string()))?
        .json::<BrowserVersion>()
        .await
        .map_err(|error| BrowserGatewayError::Cdp(error.to_string()))?;
    let (client, _events) = CdpClient::connect(&version.websocket_url).await?;
    client.send_fire_and_forget("Browser.close", serde_json::json!({}))
}

pub(crate) async fn launch_cloakbrowser(
    config: &BrowserGatewayConfig,
    session: &BrowserSession,
) -> Result<(BrowserRuntime, BrowserTarget), BrowserGatewayError> {
    let fingerprint = BrowserFingerprint::checked_metadata(&session.metadata)?;
    tokio::fs::create_dir_all(&session.browser_profile_dir)
        .await
        .map_err(|err| BrowserGatewayError::BrowserLaunch(err.to_string()))?;
    tokio::fs::create_dir_all(&session.profile_dir)
        .await
        .map_err(|err| BrowserGatewayError::BrowserLaunch(err.to_string()))?;
    let bridge_config = crate::proxy_bridge::config(session);
    if let Some(runtime) = attach_registered_runtime(&session.browser_profile_dir).await {
        if proxy::runtime_config_matches(&runtime_port_file(&session.browser_profile_dir), session)
            .await
            && (bridge_config.is_none() || runtime.0.bridge.is_some())
        {
            return Ok(runtime);
        }
        // 代理或指纹变化、以及重启后转接丢失时，必须重启浏览器使新配置生效。
        request_browser_close(runtime.0.debugging_port).await?;
        sleep(Duration::from_secs(1)).await;
    }
    let bridge = if let Some(config) = bridge_config {
        Some(crate::proxy_bridge::start(&session.browser_profile_dir, config).await?)
    } else {
        None
    };

    let executable = resolve_cloakbrowser_path(config)?;
    let debugging_port = get_free_port().await?;
    let viewport = session.viewport;
    let mut args = vec![
        format!("--remote-debugging-port={debugging_port}"),
        format!("--user-data-dir={}", session.browser_profile_dir),
        format!("--window-size={},{}", viewport.width, viewport.height),
        "--disable-dev-shm-usage".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--password-store=basic".to_string(),
        "--use-mock-keychain".to_string(),
    ];

    if session.headless {
        args.push("--headless=new".to_string());
    }
    if cfg!(target_os = "linux") && std::env::var("USER").is_ok_and(|user| user == "root") {
        args.push("--no-sandbox".to_string());
    }
    if let Some(fingerprint) = fingerprint {
        let native_major = installed_browser_major_version(config).await?;
        args.extend(
            fingerprint.launch_args(Path::new(&session.browser_profile_dir), Some(native_major)),
        );
    }
    args.extend(proxy_launch_args(
        bridge
            .as_ref()
            .map(|b| b.address.clone())
            .or_else(|| proxy::proxy_server_from_session(session)),
    ));
    // 先打开空白页，等 CDP 原生弹窗 guard 安装完成后再导航到目标页，避免页面抢先弹出系统认证框。
    args.push(browser_launch_url().to_string());

    let mut command = Command::new(&executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // 会话创建中途失败时也必须终止浏览器，避免残留进程继续锁住 profile。
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|err| {
        BrowserGatewayError::BrowserLaunch(format!(
            "启动 CloakBrowser 失败: {} ({})",
            executable.display(),
            err
        ))
    })?;

    let target = match wait_for_page_target(debugging_port, &mut child).await {
        Ok(target) => target,
        Err(error) => {
            // 两个服务实例同时启动时，另一个实例可能刚写好端口登记；失败后再接管一次。
            if let Some(runtime) = attach_registered_runtime(&session.browser_profile_dir).await {
                if proxy::runtime_config_matches(
                    &runtime_port_file(&session.browser_profile_dir),
                    session,
                )
                .await
                    && (crate::proxy_bridge::config(session).is_none()
                        || runtime.0.bridge.is_some())
                {
                    return Ok(runtime);
                }
            }
            return Err(error);
        }
    };
    write_runtime_port(session, debugging_port).await?;
    let mut runtime = BrowserRuntime::owned(debugging_port, child, &session.browser_profile_dir);
    runtime.bridge = bridge;
    Ok((runtime, target))
}

fn proxy_launch_args(server: Option<String>) -> Vec<String> {
    // Chromium 默认让 localhost 直连；OAuth 的本机回调需要保留这条规则。
    server
        .map(|server| vec![format!("--proxy-server={server}")])
        .unwrap_or_default()
}

async fn attach_registered_runtime(profile_dir: &str) -> Option<(BrowserRuntime, BrowserTarget)> {
    let port_file = runtime_port_file(profile_dir);
    let raw = tokio::fs::read_to_string(&port_file).await.ok()?;
    let Some(debugging_port) = parse_runtime_port(&raw) else {
        let _ = tokio::fs::remove_file(port_file).await;
        return None;
    };

    // 端口文件可能比 DevTools HTTP 服务先落盘，短暂重试可覆盖并发启动和服务重启竞态。
    for _ in 0..8 {
        if let Ok(target) = get_best_page_target(debugging_port).await {
            return Some((
                BrowserRuntime::attached(debugging_port, profile_dir),
                target,
            ));
        }
        sleep(Duration::from_millis(250)).await;
    }
    let _ = tokio::fs::remove_file(port_file).await;
    None
}

async fn write_runtime_port(
    session: &BrowserSession,
    debugging_port: u16,
) -> Result<(), BrowserGatewayError> {
    tokio::fs::write(
        runtime_port_file(&session.browser_profile_dir),
        serde_json::json!({
            "port": debugging_port, "settings": proxy::runtime_signature(session)
        })
        .to_string(),
    )
    .await
    .map_err(|err| {
        BrowserGatewayError::BrowserLaunch(format!("记录 CloakBrowser 调试端口失败: {err}"))
    })
}

fn runtime_port_file(profile_dir: &str) -> PathBuf {
    Path::new(profile_dir).join(RUNTIME_PORT_FILE)
}

fn parse_runtime_port(raw: &str) -> Option<u16> {
    raw.trim()
        .parse::<u16>()
        .ok()
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(raw)
                .ok()?
                .get("port")?
                .as_u64()?
                .try_into()
                .ok()
        })
        .filter(|port| *port > 0)
}

fn browser_launch_url() -> &'static str {
    SAFE_LAUNCH_URL
}

fn resolve_cloakbrowser_path(
    config: &BrowserGatewayConfig,
) -> Result<PathBuf, BrowserGatewayError> {
    if let Some(path) = &config.browser_path {
        if let Some(candidate) = first_existing_browser_binary(path_or_runtime_candidates(path)) {
            return Ok(candidate);
        }
    }

    for path in explicit_env_candidate_paths() {
        if let Some(candidate) = first_existing_browser_binary(path_or_runtime_candidates(&path)) {
            return Ok(candidate);
        }
    }

    first_existing_browser_binary(candidate_paths()).ok_or_else(|| {
        BrowserGatewayError::BrowserRuntimeUnavailable(
            "未检测到 CloakBrowser；源码运行请在项目根目录执行 node scripts/ensure-cloakbrowser-runtime.mjs，或将 CLOAK_BROWSER_PATH 设置为浏览器可执行文件或运行时目录".to_string(),
        )
    })
}

pub(crate) async fn installed_browser_major_version(
    config: &BrowserGatewayConfig,
) -> Result<u16, BrowserGatewayError> {
    let executable = resolve_cloakbrowser_path(config)?;
    let output = timeout(
        Duration::from_secs(5),
        Command::new(executable).arg("--version").output(),
    )
    .await
    .map_err(|_| BrowserGatewayError::BrowserRuntimeUnavailable("读取浏览器版本超时".into()))?
    .map_err(|error| BrowserGatewayError::BrowserRuntimeUnavailable(error.to_string()))?;
    if !output.status.success() {
        return Err(BrowserGatewayError::BrowserRuntimeUnavailable(
            "无法读取浏览器版本".into(),
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find_map(|part| part.split('.').next()?.parse::<u16>().ok())
        .filter(|version| *version > 0)
        .ok_or_else(|| BrowserGatewayError::BrowserRuntimeUnavailable("浏览器版本格式无效".into()))
}

fn candidate_paths() -> Vec<PathBuf> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut bundled = env_bundle_candidate_paths();
    bundled.extend(local_bundle_candidate_paths(&root));
    if cfg!(target_os = "macos") {
        bundled.extend([
            "/Applications/CloakBrowser.app/Contents/MacOS/CloakBrowser".into(),
            "/Applications/Cloak Browser.app/Contents/MacOS/CloakBrowser".into(),
        ]);
        bundled.extend(cache_candidate_paths());
        return bundled;
    }
    if cfg!(target_os = "windows") {
        bundled.extend([
            r"C:\Program Files\CloakBrowser\Application\cloakbrowser.exe".into(),
            r"C:\Program Files (x86)\CloakBrowser\Application\cloakbrowser.exe".into(),
        ]);
        bundled.extend(cache_candidate_paths());
        return bundled;
    }
    bundled.extend([
        "/usr/bin/cloakbrowser".into(),
        "/usr/local/bin/cloakbrowser".into(),
        "/opt/cloakbrowser/chrome".into(),
    ]);
    bundled.extend(cache_candidate_paths());
    bundled
}

fn local_bundle_candidate_paths(root: &Path) -> Vec<PathBuf> {
    let mut candidates = bundled_cloakbrowser_candidates(root);
    // 源码开发复用安装脚本的默认输出；安装目录中的运行时仍然优先。
    candidates.extend(bundled_cloakbrowser_candidates(
        &root.join("dist-local/MediaGen-local"),
    ));
    candidates
}

fn env_bundle_candidate_paths() -> Vec<PathBuf> {
    ["MEDIAGEN_CLOAK_BROWSER_DIR", "CLOAK_BROWSER_BUNDLE_DIR"]
        .iter()
        .filter_map(|name| std::env::var_os(name).map(PathBuf::from))
        .flat_map(|path| path_or_runtime_candidates(&path))
        .collect()
}

fn explicit_env_candidate_paths() -> Vec<PathBuf> {
    ["CLOAK_BROWSER_PATH", "CLOAKBROWSER_BINARY_PATH"]
        .iter()
        .filter_map(|name| std::env::var_os(name).map(PathBuf::from))
        .collect()
}

fn cache_candidate_paths() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(cache_root()) else {
        return Vec::new();
    };
    let mut versions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if is_cloakbrowser_cache_version(&path) {
            versions.push(path);
        }
    }
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    versions.into_iter().map(cache_runtime_binary).collect()
}

fn cache_root() -> PathBuf {
    std::env::var_os("CLOAKBROWSER_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".cloakbrowser"))
        })
        .unwrap_or_else(|| PathBuf::from(".cloakbrowser"))
}

fn is_cloakbrowser_cache_version(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("chromium-"))
}

fn cache_runtime_binary(dir: PathBuf) -> PathBuf {
    if cfg!(target_os = "macos") {
        return dir.join("Chromium.app/Contents/MacOS/Chromium");
    }
    if cfg!(target_os = "windows") {
        return dir.join("chrome.exe");
    }
    dir.join("chrome")
}

async fn wait_for_page_target(
    debugging_port: u16,
    child: &mut Child,
) -> Result<BrowserTarget, BrowserGatewayError> {
    for _ in 0..80 {
        if let Ok(target) = get_best_page_target(debugging_port).await {
            return Ok(target);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|err| BrowserGatewayError::BrowserLaunch(err.to_string()))?
        {
            return Err(BrowserGatewayError::BrowserLaunch(format!(
                "CloakBrowser 启动后提前退出（{status}），浏览器资料目录可能正被其他进程占用"
            )));
        }
        sleep(Duration::from_millis(250)).await;
    }
    Err(BrowserGatewayError::Cdp(
        "等待 CloakBrowser 调试端口超时".to_string(),
    ))
}

async fn get_free_port() -> Result<u16, BrowserGatewayError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| BrowserGatewayError::BrowserLaunch(err.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|err| BrowserGatewayError::BrowserLaunch(err.to_string()))?
        .port();
    drop(listener);
    Ok(port)
}

#[allow(dead_code)]
fn path_exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
