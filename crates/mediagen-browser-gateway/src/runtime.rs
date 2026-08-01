use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use mediagen_core::{
    bundled_cloakbrowser_candidates, first_existing_browser_binary, path_or_runtime_candidates,
};
use serde::Deserialize;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use crate::fingerprint::BrowserFingerprint;
use crate::proxy;
use crate::{BrowserGatewayConfig, BrowserGatewayError, BrowserSession};

const SAFE_LAUNCH_URL: &str = "about:blank";
const RUNTIME_PORT_FILE: &str = ".mediagen-browser-port";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BrowserTarget {
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub websocket_url: String,
}

#[derive(Clone)]
pub(crate) struct BrowserRuntime {
    pub debugging_port: u16,
    child: Arc<Mutex<Option<Child>>>,
    port_file: PathBuf,
}

impl BrowserRuntime {
    pub(crate) fn owned(debugging_port: u16, child: Child, profile_dir: &str) -> Self {
        Self {
            debugging_port,
            child: Arc::new(Mutex::new(Some(child))),
            port_file: runtime_port_file(profile_dir),
        }
    }

    fn attached(debugging_port: u16, profile_dir: &str) -> Self {
        Self {
            debugging_port,
            child: Arc::new(Mutex::new(None)),
            port_file: runtime_port_file(profile_dir),
        }
    }

    pub(crate) async fn stop(&self) {
        let mut child = self.child.lock().await;
        // 只有当前服务亲自启动的进程才能直接 kill；跨重启接管的进程保留端口登记，
        // 后续任务仍可继续接管，不会再次因为资料目录锁报 exit status 21。
        if let Some(mut child) = child.take() {
            let _ = child.kill().await;
            let _ = tokio::fs::remove_file(&self.port_file).await;
        }
    }
}

pub(crate) async fn launch_cloakbrowser(
    config: &BrowserGatewayConfig,
    session: &BrowserSession,
) -> Result<(BrowserRuntime, BrowserTarget), BrowserGatewayError> {
    tokio::fs::create_dir_all(&session.profile_dir)
        .await
        .map_err(|err| BrowserGatewayError::BrowserLaunch(err.to_string()))?;
    if let Some(runtime) = attach_registered_runtime(&session.profile_dir).await {
        return Ok(runtime);
    }

    let executable = resolve_cloakbrowser_path(config)?;
    let debugging_port = get_free_port().await?;
    let viewport = session.viewport;
    let mut args = vec![
        format!("--remote-debugging-port={debugging_port}"),
        format!("--user-data-dir={}", session.profile_dir),
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
    if let Some(fingerprint) = BrowserFingerprint::from_metadata(&session.metadata) {
        args.push(format!("--user-agent={}", fingerprint.user_agent()));
    }
    if let Some(proxy_server) = proxy::proxy_server_from_session(session) {
        args.push(format!("--proxy-server={proxy_server}"));
    }
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
            if let Some(runtime) = attach_registered_runtime(&session.profile_dir).await {
                return Ok(runtime);
            }
            return Err(error);
        }
    };
    write_runtime_port(&session.profile_dir, debugging_port).await?;
    Ok((
        BrowserRuntime::owned(debugging_port, child, &session.profile_dir),
        target,
    ))
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
    profile_dir: &str,
    debugging_port: u16,
) -> Result<(), BrowserGatewayError> {
    tokio::fs::write(runtime_port_file(profile_dir), debugging_port.to_string())
        .await
        .map_err(|err| {
            BrowserGatewayError::BrowserLaunch(format!("记录 CloakBrowser 调试端口失败: {err}"))
        })
}

fn runtime_port_file(profile_dir: &str) -> PathBuf {
    Path::new(profile_dir).join(RUNTIME_PORT_FILE)
}

fn parse_runtime_port(raw: &str) -> Option<u16> {
    raw.trim().parse::<u16>().ok().filter(|port| *port > 0)
}

fn browser_launch_url() -> &'static str {
    SAFE_LAUNCH_URL
}

pub(crate) async fn get_best_page_target(
    debugging_port: u16,
) -> Result<BrowserTarget, BrowserGatewayError> {
    let targets = get_page_targets(debugging_port).await?;
    best_page_target(&targets)
}

pub(crate) async fn get_page_target(
    debugging_port: u16,
    target_id: &str,
) -> Result<Option<BrowserTarget>, BrowserGatewayError> {
    Ok(get_page_targets(debugging_port)
        .await?
        .into_iter()
        .find(|target| target.id == target_id))
}

pub(crate) async fn create_page_target(
    debugging_port: u16,
) -> Result<BrowserTarget, BrowserGatewayError> {
    reqwest::Client::new()
        .put(format!(
            "http://127.0.0.1:{debugging_port}/json/new?{SAFE_LAUNCH_URL}"
        ))
        .send()
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
        .error_for_status()
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
        .json::<BrowserTarget>()
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))
}

pub(crate) async fn close_page_target(
    debugging_port: u16,
    target_id: &str,
) -> Result<(), BrowserGatewayError> {
    if target_id.trim().is_empty() {
        return Ok(());
    }
    reqwest::get(format!(
        "http://127.0.0.1:{debugging_port}/json/close/{target_id}"
    ))
    .await
    .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
    .error_for_status()
    .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?;
    Ok(())
}

async fn get_page_targets(debugging_port: u16) -> Result<Vec<BrowserTarget>, BrowserGatewayError> {
    let url = format!("http://127.0.0.1:{debugging_port}/json/list");
    reqwest::get(url)
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
        .json::<Vec<BrowserTarget>>()
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))
}

fn best_page_target(targets: &[BrowserTarget]) -> Result<BrowserTarget, BrowserGatewayError> {
    targets
        .iter()
        .find(|target| is_useful_page_url(&target.url))
        .or_else(|| targets.first())
        .cloned()
        .ok_or_else(|| BrowserGatewayError::Cdp("没有找到可控制的浏览器页面".to_string()))
}

pub(crate) fn is_useful_page_url(url: &str) -> bool {
    !url.is_empty() && url != "about:blank" && !url.starts_with("chrome://")
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
            "未检测到 CloakBrowser，请安装后设置 CLOAK_BROWSER_PATH".to_string(),
        )
    })
}

fn candidate_paths() -> Vec<PathBuf> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut bundled = env_bundle_candidate_paths();
    bundled.extend(bundled_cloakbrowser_candidates(&root));
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
mod tests {
    use super::{browser_launch_url, parse_runtime_port};

    #[test]
    fn browser_starts_blank_before_cdp_guards_are_ready() {
        assert_eq!(browser_launch_url(), "about:blank");
    }

    #[test]
    fn runtime_port_parser_rejects_invalid_registry_values() {
        assert_eq!(parse_runtime_port("52500\n"), Some(52500));
        assert_eq!(parse_runtime_port("0"), None);
        assert_eq!(parse_runtime_port("not-a-port"), None);
    }
}
