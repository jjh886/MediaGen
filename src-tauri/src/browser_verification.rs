use std::ffi::OsStr;
use std::path::PathBuf;

use headless_chrome::{Browser, LaunchOptions};

use crate::browser_manager::{fix_chrome_exit_type_pub, merge_userinfo, LoginResult};
use crate::engine::browser_engine::{dump_cookies_as_json, graceful_shutdown_owned_browser};
use crate::types::LoginDetection;

/// 人工验证配置
pub struct VerificationConfig {
    pub chrome_path: Option<String>,
    pub profile_dir: PathBuf,
    /// 平台页面 URL。
    pub page_url: String,
    /// 恢复的 cookies
    pub cookies_json: String,
    /// 要重发的消息内容（触发验证用）
    pub replay_steps: Vec<VerificationStep>,
    /// 登录检测配置
    pub detection: LoginDetection,
    pub userinfo_script: Option<String>,
    pub userinfo_await_promise: bool,
}

/// 验证流程中需要重放的操作步骤
#[derive(Clone)]
pub struct VerificationStep {
    /// JS 脚本内容
    pub script: String,
    pub await_promise: bool,
    /// 执行后等待毫秒数
    pub wait_ms: u64,
}

/// 验证结果
#[derive(Debug)]
pub enum VerificationOutcome {
    /// 重发后没触发验证，账号正常
    NoVerificationNeeded(LoginResult),
    /// 用户完成验证，账号恢复
    Verified(LoginResult),
    /// 超时未处理
    Timeout,
    /// 浏览器启动或操作失败
    Failed(String),
}

/// 人工验证处理原子能力
///
/// 流程：
/// 1. 用账号 profile 打开有头浏览器（屏幕外不可见）
/// 2. 恢复 cookie + 导航到平台页
/// 3. 重放上一次的操作（重发一模一样的消息）
/// 4. 监听响应：如果没触发验证 → 直接恢复账号，关闭浏览器
/// 5. 如果触发验证 → 把浏览器移到可见区域，等用户处理
/// 6. 10 分钟内用户处理完 → 恢复账号；超时 → 账号暂停
///
/// TODO(@jijunhong): 验证页面的具体元素（验证码弹窗选择器、完成按钮等）待补充，
/// 当前依赖 window.__mediagen.status 变化和用户手动关闭浏览器来判断验证完成。
pub fn open_for_verification(config: VerificationConfig) -> VerificationOutcome {
    let profile = config.profile_dir.clone();
    std::fs::create_dir_all(&profile).ok();
    for lock in &["SingletonLock", "SingletonSocket", "SingletonCookie"] {
        let _ = std::fs::remove_file(profile.join(lock));
    }
    fix_chrome_exit_type_pub(&profile);

    let (browser, tab) = match launch_offscreen_browser(&config, profile) {
        Ok(r) => r,
        Err(e) => return VerificationOutcome::Failed(e),
    };

    let outcome = run_verification_flow(&tab, &config);
    // 不论成功/失败/超时，统一走优雅关闭（与登录、任务结束复用同一路径）
    graceful_shutdown_owned_browser(browser, tab, "verification");
    outcome
}

fn run_verification_flow(
    tab: &std::sync::Arc<headless_chrome::Tab>,
    config: &VerificationConfig,
) -> VerificationOutcome {
    if let Err(e) = tab.navigate_to(&config.page_url) {
        return VerificationOutcome::Failed(format!("导航失败: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_secs(3));
    restore_cookies(tab, &config.cookies_json, &config.page_url);
    if let Err(e) = tab.navigate_to(&config.page_url) {
        return VerificationOutcome::Failed(format!("导航失败: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_secs(5));

    // 重放操作步骤（重发消息触发验证）
    for step in &config.replay_steps {
        let _ = tab.evaluate(&step.script, step.await_promise);
        if step.wait_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(step.wait_ms));
        }
    }
    tracing::info!("验证流程：已重放操作步骤，等待响应");

    wait_for_verification_trigger(tab, config)
}

fn launch_offscreen_browser(
    config: &VerificationConfig,
    profile: PathBuf,
) -> Result<(Browser, std::sync::Arc<headless_chrome::Tab>), String> {
    let chrome_pb = config.chrome_path.as_ref().map(PathBuf::from);
    let default_args_to_ignore: Vec<&OsStr> = [
        "--enable-automation",
        "--disable-extensions",
        "--disable-default-apps",
        "--disable-component-extensions-with-background-pages",
        "--disable-client-side-phishing-detection",
        "--disable-sync",
        "--disable-background-networking",
        "--metrics-recording-only",
        "--use-mock-keychain",
        "--password-store=basic",
        "--disable-popup-blocking",
    ]
    .iter()
    .map(OsStr::new)
    .collect();
    let launch_opts = LaunchOptions::default_builder()
        .path(chrome_pb)
        .user_data_dir(Some(profile))
        .window_size(Some((1400, 900)))
        .idle_browser_timeout(std::time::Duration::from_secs(660))
        .ignore_default_args(default_args_to_ignore)
        .args({
            let args: Vec<&OsStr> = vec![
                OsStr::new("--remote-allow-origins=*"),
                OsStr::new("--no-first-run"),
                OsStr::new("--no-default-browser-check"),
                OsStr::new("--disable-session-crashed-bubble"),
                OsStr::new("--window-position=-32000,-32000"),
            ];
            args
        })
        .headless(false)
        .sandbox(true)
        .build()
        .map_err(|e| format!("启动参数错误: {e}"))?;

    let browser = Browser::new(launch_opts).map_err(|e| format!("Chrome 启动失败: {e}"))?;
    #[allow(deprecated)]
    let tab = browser
        .wait_for_initial_tab()
        .map_err(|e| format!("获取标签页失败: {e}"))?;
    Ok((browser, tab))
}

fn restore_cookies(tab: &headless_chrome::Tab, cookies_json: &str, url: &str) {
    if cookies_json.is_empty() {
        return;
    }
    let Ok(cookies) = serde_json::from_str::<Vec<serde_json::Value>>(cookies_json) else {
        return;
    };
    use headless_chrome::protocol::cdp::Network;
    for c in &cookies {
        let _ = tab.set_cookies(vec![Network::CookieParam {
            name: c["name"].as_str().unwrap_or("").to_string(),
            value: c["value"].as_str().unwrap_or("").to_string(),
            url: Some(url.to_string()),
            domain: c["domain"].as_str().map(String::from),
            path: c["path"].as_str().map(String::from),
            secure: c["secure"].as_bool(),
            http_only: c["httpOnly"].as_bool(),
            same_site: None,
            expires: None,
            priority: None,
            same_party: None,
            source_scheme: None,
            source_port: None,
            partition_key: None,
        }]);
    }
}

fn get_mediagen_status(tab: &headless_chrome::Tab) -> Option<String> {
    tab.evaluate(
        "(window.__mediagen && window.__mediagen.status) || 'unknown'",
        false,
    )
    .ok()
    .and_then(|r| r.value)
    .and_then(|v| v.as_str().map(String::from))
}

/// 等待 30 秒看是否触发验证，然后决定后续流程
fn wait_for_verification_trigger(
    tab: &headless_chrome::Tab,
    config: &VerificationConfig,
) -> VerificationOutcome {
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if tab.evaluate("1+1", false).is_err() {
            return VerificationOutcome::NoVerificationNeeded(LoginResult::default());
        }
        match get_mediagen_status(tab).as_deref() {
            Some("done") => {
                tracing::info!("重发后未触发验证，生成成功，账号正常");
                return collect_result(tab, config);
            }
            Some("needs_verification") | Some("error") => {
                tracing::warn!("重发后触发验证，显示浏览器窗口等待用户处理");
                return show_and_wait_for_user(tab, config);
            }
            _ => {}
        }
    }
    tracing::info!("30 秒内未触发验证，继续等待生成完成");
    collect_result(tab, config)
}

/// 把浏览器移到可见区域，等用户处理验证（最长 10 分钟）
fn show_and_wait_for_user(
    tab: &headless_chrome::Tab,
    config: &VerificationConfig,
) -> VerificationOutcome {
    let _ = tab.set_bounds(headless_chrome::types::Bounds::Normal {
        left: Some(100),
        top: Some(100),
        width: Some(1400.0),
        height: Some(900.0),
    });

    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if tab.evaluate("1+1", false).is_err() {
            return collect_result(tab, config);
        }
        if get_mediagen_status(tab).as_deref() == Some("done") {
            tracing::info!("用户完成验证，生成成功");
            return collect_result(tab, config);
        }
    }

    tracing::warn!("验证超时（10 分钟），账号暂停使用");
    VerificationOutcome::Timeout
}

/// 收集登录信息并返回
fn collect_result(tab: &headless_chrome::Tab, config: &VerificationConfig) -> VerificationOutcome {
    let mut result = LoginResult {
        logged_in: true,
        ..Default::default()
    };
    if let Some(ref script) = config.userinfo_script {
        if let Ok(eval) = tab.evaluate(script, config.userinfo_await_promise) {
            if let Some(info) = eval.value.as_ref().and_then(|v| {
                v.as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .or_else(|| Some(v.clone()))
            }) {
                merge_userinfo(&mut result, &info);
            }
        }
    }
    // 统一走 CDP 原始 getAllCookies，规避 sourcePort=-1 的 u32 反序列化崩溃
    result.cookies_json = dump_cookies_as_json(tab);
    VerificationOutcome::Verified(result)
}
