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
    child: Arc<Mutex<Child>>,
}

impl BrowserRuntime {
    pub(crate) fn new(debugging_port: u16, child: Child) -> Self {
        Self {
            debugging_port,
            child: Arc::new(Mutex::new(child)),
        }
    }

    pub(crate) async fn stop(&self) {
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
    }
}

pub(crate) async fn launch_cloakbrowser(
    config: &BrowserGatewayConfig,
    session: &BrowserSession,
) -> Result<(BrowserRuntime, BrowserTarget), BrowserGatewayError> {
    let executable = resolve_cloakbrowser_path(config)?;
    tokio::fs::create_dir_all(&session.profile_dir)
        .await
        .map_err(|err| BrowserGatewayError::BrowserLaunch(err.to_string()))?;

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

    let target = wait_for_page_target(debugging_port, &mut child).await?;
    Ok((BrowserRuntime::new(debugging_port, child), target))
}

fn browser_launch_url() -> &'static str {
    SAFE_LAUNCH_URL
}

pub(crate) async fn get_best_page_target(
    debugging_port: u16,
) -> Result<BrowserTarget, BrowserGatewayError> {
    let url = format!("http://127.0.0.1:{debugging_port}/json/list");
    let targets = reqwest::get(url)
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
        .json::<Vec<BrowserTarget>>()
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?;

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
    use super::browser_launch_url;

    #[test]
    fn browser_starts_blank_before_cdp_guards_are_ready() {
        assert_eq!(browser_launch_url(), "about:blank");
    }
}
