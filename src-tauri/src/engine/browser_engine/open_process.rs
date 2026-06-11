use std::ffi::OsStr;
use std::path::PathBuf;

use headless_chrome::{Browser, LaunchOptions};

use super::inject_stealth;
use crate::error::AppError;

/// 基于 profile_dir hash 从常见分辨率集里选一个窗口尺寸，避免所有实例都固定 1400x900。
fn pick_window_size(profile_dir: &str) -> (u32, u32) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    profile_dir.hash(&mut hasher);
    let h = hasher.finish();
    let choices: [(u32, u32); 5] = [
        (1280, 800),
        (1366, 768),
        (1440, 900),
        (1536, 864),
        (1600, 900),
    ];
    choices[(h as usize) % choices.len()]
}

/// 在 blocking 线程中启动 Chrome 进程
pub(super) async fn spawn_chrome_process(
    chrome_pb: Option<PathBuf>,
    user_data_dir: Option<PathBuf>,
    headless: bool,
    offscreen: bool,
    profile_dir_str: String,
) -> Result<(Browser, std::sync::Arc<headless_chrome::Tab>), AppError> {
    let ignore_args: Vec<&OsStr> = vec![
        OsStr::new("--enable-automation"),
        OsStr::new("--disable-extensions"),
        OsStr::new("--disable-default-apps"),
        OsStr::new("--disable-component-extensions-with-background-pages"),
        OsStr::new("--disable-client-side-phishing-detection"),
        OsStr::new("--metrics-recording-only"),
        OsStr::new("--use-mock-keychain"),
        OsStr::new("--password-store=basic"),
        OsStr::new("--disable-popup-blocking"),
        OsStr::new("--disable-sync"),
    ];

    let window_size = pick_window_size(&profile_dir_str);

    match tokio::task::spawn_blocking(move || {
        // 保留 --remote-allow-origins=*：CDP 连接必需，去掉会导致 headless_chrome 连不上
        let mut extra_args: Vec<&OsStr> = vec![
            OsStr::new("--remote-allow-origins=*"),
            OsStr::new("--no-first-run"),
            OsStr::new("--no-default-browser-check"),
            OsStr::new("--disable-session-crashed-bubble"),
            // 关键反自动化规避：禁用 AutomationControlled feature，让 stealth.js
            // 覆盖 navigator.webdriver 真正生效；缺这条会触发 Google 等强校验站点
            // 在 session 建立后立即登出。
            // --test-type 用于抑制"您使用的是不受支持的命令行标记"黄条
            // （详见 browser_manager.rs 同位置注释）。
            OsStr::new("--test-type"),
            OsStr::new("--disable-blink-features=AutomationControlled"),
        ];
        if headless {
            extra_args.push(OsStr::new("--headless=new"));
        }
        if offscreen {
            // 把窗口移到屏幕外（常见于人工验证场景：先隐藏，触发验证后再通过 show_window 拉回来）
            extra_args.push(OsStr::new("--window-position=-32000,-32000"));
        }
        if cfg!(target_os = "windows") {
            extra_args.push(OsStr::new("--disable-gpu"));
        }

        tracing::info!(
            chrome_path = ?chrome_pb,
            user_data_dir = ?user_data_dir,
            headless,
            offscreen,
            window_size = ?window_size,
            "准备启动 Chrome 进程"
        );

        let launch_opts = LaunchOptions::default_builder()
            .path(chrome_pb)
            .user_data_dir(user_data_dir)
            .window_size(Some(window_size))
            .idle_browser_timeout(std::time::Duration::from_secs(86_400))
            .ignore_default_args(ignore_args)
            .args(extra_args)
            .headless(false)
            .sandbox(true)
            .ignore_certificate_errors(false)
            .build()
            .map_err(|e| AppError::BrowserError(format!("Chrome 启动参数错误: {e}")))?;

        tracing::info!("调用 Browser::new() ...");
        let browser = Browser::new(launch_opts)
            .map_err(|e| AppError::BrowserError(format!("Chrome 启动失败: {e}")))?;
        tracing::info!("Browser::new() 成功，等待可复用启动标签页");

        // 无头和有头都先等 Chrome 自己生成的初始 tab 注册，再扫描当前窗口里
        // 所有已注册 tab。只要有空白/新标签启动页就沿用；找不到才创建新 tab。
        // 这样不会误抢已有业务页，也避免调试时额外堆出一个业务标签。
        #[allow(deprecated)]
        let _ = browser.wait_for_initial_tab();
        let startup_tab = {
            let tabs = browser.get_tabs().lock().unwrap();
            tabs.iter()
                .find(|tab| {
                    let url = tab.get_url();
                    let url = url.trim_end_matches('/');
                    url.is_empty()
                        || url == "about:blank"
                        || url == "chrome://newtab"
                        || url.starts_with("chrome://new-tab-page")
                        || url.starts_with("chrome-search://local-ntp")
                        || url.starts_with("chrome-untrusted://new-tab-page")
                })
                .cloned()
        };
        let tab = match startup_tab {
            Some(tab) => tab,
            _ => browser
                .new_tab()
                .map_err(|e| AppError::BrowserError(format!("创建标签页失败: {e}")))?,
        };

        if let Err(e) = tab.activate() {
            tracing::debug!(error = %e, "激活受控标签页失败，继续执行");
        }
        if let Err(e) = tab.bring_to_front() {
            tracing::debug!(error = %e, "前置受控标签页失败，继续执行");
        }

        inject_stealth(&tab, &profile_dir_str);

        Ok::<_, AppError>((browser, tab))
    })
    .await
    {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(AppError::BrowserError(format!("open 任务失败: {e}"))),
    }
}
