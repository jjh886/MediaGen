use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;

use headless_chrome::{Browser, LaunchOptions};
use serde::{Deserialize, Serialize};

use crate::engine::browser_engine::{
    dump_cookies_as_json, get_all_cookies_raw, graceful_shutdown_owned_browser,
    kill_chrome_by_profile, BrowserEngine,
};
use crate::types::{BrowserLifecycle, LoginDetection};

/// 登录后 Chrome 关闭完成到下一次任务启动 headless Chrome 之间的冷却区间（毫秒）。
/// 随机取 [2000, 4000]，避免 Google 把"前台 Chrome 退出 + 紧邻启动 headless"识别为
/// 自动化会话搬家。配合 graceful_shutdown 让 Chrome 先把 session 写进 profile SQLite。
const PERSISTENT_LOGIN_COOLDOWN_MIN_MS: u64 = 2_000;
const PERSISTENT_LOGIN_COOLDOWN_RANGE_MS: u64 = 2_001;

// ── 公开类型 ──────────────────────────────────────────────────────

/// 登录检测 + 用户信息提取的统一结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginResult {
    pub logged_in: bool,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub credits: i32,
    #[serde(default)]
    pub cookies_json: String,
    /// persistent 模式下 Chrome 被交给 BrowserEngine 时分配的 session_id，
    /// per_task 模式下为空字符串。
    #[serde(default)]
    pub session_id: String,
}

/// 传给 open_and_wait_for_login 的配置，全部从插件 manifest 和脚本文件读取
pub struct BrowserLoginConfig {
    pub chrome_path: Option<String>,
    pub profile_dir: PathBuf,
    pub login_url: String,
    pub detection: LoginDetection,
    /// 登录成功后提取用户信息的 JS 脚本内容（已从文件读取）
    pub userinfo_script: Option<String>,
    pub userinfo_await_promise: bool,
}

// ── 统一的浏览器登录流程 ─────────────────────────────────────────

/// 启动 Chrome、导航到登录页、轮询检测登录状态、提取用户信息和 cookies。
/// 完全通用——所有平台特定逻辑通过 config 参数注入。
///
/// 登录完成后无论 lifecycle 都走 `graceful_shutdown_owned_browser` 优雅关闭本次有头 Chrome
/// （CDP `Page.close` → SIGTERM → 等进程退出），让 Chrome 把 session/cookies 写进 profile
/// SQLite 再退出。`lifecycle = Persistent` 额外在关闭后随机冷却 2–4 秒，让 Google 侧看到的是
/// "浏览器 session 正常结束"而不是"紧邻的进程搬家"——任务来了再以 `--headless=new` 启动并
/// 靠 profile 持久化恢复 cookies，风控更友好。
///
/// `engine` 参数保留不用，维持 API 稳定：未来若再改成"登录后交给 engine"可以直接插回去。
pub fn open_and_wait_for_login(
    config: BrowserLoginConfig,
    lifecycle: BrowserLifecycle,
    _engine: Option<Arc<BrowserEngine>>,
) -> Result<LoginResult, String> {
    prepare_profile(&config)?;

    let (browser, tab) = launch_login_browser(&config)?;
    let result = run_login_flow(&tab, &config);

    graceful_shutdown_owned_browser(browser, tab, "login");

    if matches!(lifecycle, BrowserLifecycle::Persistent) {
        let logged_in = result.as_ref().map(|r| r.logged_in).unwrap_or(false);
        let cooldown_ms = persistent_login_cooldown_ms();
        tracing::info!(
            logged_in,
            cooldown_ms,
            "persistent 平台登录关闭后冷却，避免 Google 识别为 session 搬家"
        );
        std::thread::sleep(std::time::Duration::from_millis(cooldown_ms));
    }

    result
}

/// 伪随机返回登录关闭后冷却的毫秒数，范围 [2000, 4000]。
/// 用系统时间的纳秒低位取模，避免引入 rand 依赖。
fn persistent_login_cooldown_ms() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    PERSISTENT_LOGIN_COOLDOWN_MIN_MS + (nanos % PERSISTENT_LOGIN_COOLDOWN_RANGE_MS)
}

fn run_login_flow(
    tab: &Arc<headless_chrome::Tab>,
    config: &BrowserLoginConfig,
) -> Result<LoginResult, String> {
    tab.navigate_to(&config.login_url)
        .map_err(|e| format!("导航到 {} 失败: {e}", config.login_url))?;

    std::thread::sleep(std::time::Duration::from_secs(8));
    tracing::info!("Chrome 已打开，等待用户登录...");

    let mut result = poll_login_status(tab, config)?;
    extract_userinfo_if_configured(tab, config, &mut result);

    // 用 CDP 原始 getAllCookies 规避 sourcePort=-1 的类型崩溃
    result.cookies_json = dump_cookies_as_json(tab);

    tracing::info!(user_id = %result.user_id, username = %result.username, "用户信息提取完成");
    Ok(result)
}

fn prepare_profile(config: &BrowserLoginConfig) -> Result<(), String> {
    // 与 browser_engine::launch_new_chrome 对齐：
    // 先杀掉同 profile 的残留 Chrome 进程，否则 Chrome 的 singleton 规则会
    // 让 Windows 静默转发（不开新窗口）、macOS 弹多次 profile-error 对话框。
    // 空路径会被内部 short-circuit，add_account_start 走新 UUID 目录不会匹配到进程。
    kill_chrome_by_profile(&config.profile_dir.to_string_lossy());
    std::fs::create_dir_all(&config.profile_dir).map_err(|e| e.to_string())?;
    for lock in &["SingletonLock", "SingletonSocket", "SingletonCookie"] {
        let _ = std::fs::remove_file(config.profile_dir.join(lock));
    }
    fix_chrome_exit_type(&config.profile_dir);
    Ok(())
}

fn launch_login_browser(
    config: &BrowserLoginConfig,
) -> Result<(Browser, Arc<headless_chrome::Tab>), String> {
    tracing::info!(profile_dir = %config.profile_dir.display(), login_url = %config.login_url, "启动 Chrome 登录窗口");
    let chrome_pb = config.chrome_path.as_ref().map(PathBuf::from);
    let ignore_args: Vec<&OsStr> = vec![
        OsStr::new("--enable-automation"),
        OsStr::new("--disable-extensions"),
        OsStr::new("--disable-default-apps"),
        OsStr::new("--disable-component-extensions-with-background-pages"),
        OsStr::new("--disable-client-side-phishing-detection"),
        OsStr::new("--disable-sync"),
        OsStr::new("--disable-background-networking"),
        OsStr::new("--metrics-recording-only"),
        OsStr::new("--use-mock-keychain"),
        OsStr::new("--password-store=basic"),
        OsStr::new("--disable-popup-blocking"),
    ];
    let extra_args: Vec<&OsStr> = vec![
        OsStr::new("--remote-allow-origins=*"),
        OsStr::new("--no-first-run"),
        OsStr::new("--no-default-browser-check"),
        OsStr::new("--disable-session-crashed-bubble"),
        // 关键反自动化规避：禁用 AutomationControlled feature，
        // 否则 Chrome 会把 navigator.webdriver 以 configurable:false 暴露，
        // stealth.js 里 defineProperty 覆盖会静默失败 → Google 在 OAuth 回调时
        // 检测到 webdriver=true 直接 sign-out（"登录后一下子就自动退出"）。
        // 用 --test-type 抑制"您使用的是不受支持的命令行标记"黄条：
        // Chromium 在 bad_flags_prompt.cc 中检测到 kTestType 开关即跳过告警。
        // 注意：必须用裸 --test-type，不能用 --test-type=webdriver（后者会
        // 让 Chrome 主动把 navigator.webdriver 置 true，与此处目的相反）。
        OsStr::new("--test-type"),
        OsStr::new("--disable-blink-features=AutomationControlled"),
    ];
    let launch_opts = LaunchOptions::default_builder()
        .path(chrome_pb)
        .user_data_dir(Some(config.profile_dir.clone()))
        .window_size(Some((1400, 900)))
        .idle_browser_timeout(std::time::Duration::from_secs(660))
        .ignore_default_args(ignore_args)
        .args(extra_args)
        .headless(false)
        .sandbox(true)
        .ignore_certificate_errors(false)
        .build()
        .map_err(|e| format!("Chrome 启动参数错误: {e}"))?;

    let browser = Browser::new(launch_opts).map_err(|e| format!("Chrome 启动失败: {e}"))?;
    #[allow(deprecated)]
    let tab = browser
        .wait_for_initial_tab()
        .map_err(|e| format!("获取标签页失败: {e}"))?;
    Ok((browser, tab))
}

fn poll_login_status(
    tab: &headless_chrome::Tab,
    config: &BrowserLoginConfig,
) -> Result<LoginResult, String> {
    for attempt in 0..200u32 {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if tab.evaluate("document.readyState", false).is_err() {
            continue;
        }

        let detected = match &config.detection {
            LoginDetection::CdpCookies { cookie_names } => check_via_cdp_cookies(tab, cookie_names),
            LoginDetection::JsScript {
                script_file: _,
                await_promise,
                script_content,
            } => match script_content {
                Some(script) => eval_login_script(tab, script, *await_promise),
                None => Err("登录检测脚本未加载".to_string()),
            },
        };

        match detected {
            Ok(Some(r)) => {
                tracing::info!(attempt, "检测到登录成功");
                return Ok(r);
            }
            Ok(None) => {}
            Err(e) => {
                tracing::debug!(attempt, error = %e, "检测出错，继续重试");
            }
        }
    }
    Err("登录超时（10 分钟），请重试".to_string())
}

fn extract_userinfo_if_configured(
    tab: &headless_chrome::Tab,
    config: &BrowserLoginConfig,
    result: &mut LoginResult,
) {
    let Some(ref script) = config.userinfo_script else {
        return;
    };
    match tab.evaluate(script, config.userinfo_await_promise) {
        Ok(eval_result) => {
            if let Some(info) = eval_result.value.as_ref().and_then(|v| {
                v.as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .or_else(|| Some(v.clone()))
            }) {
                merge_userinfo(result, &info);
                tracing::debug!("userinfo 脚本提取成功");
            } else {
                tracing::warn!("userinfo 脚本返回值为空");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "userinfo 脚本执行失败");
        }
    }
}

// ── 内部辅助（全部通用，不含任何平台特定逻辑）──────────────────

/// 通过 CDP 协议检查 httpOnly cookies。复用 `get_all_cookies_raw`，与备份路径走同一条
/// 原始 CDP 调用，避免 `sourcePort=-1` 导致登录检测长期误判为"未登录"。
fn check_via_cdp_cookies(
    tab: &headless_chrome::Tab,
    cookie_names: &[String],
) -> Result<Option<LoginResult>, String> {
    let Some(cookies) = get_all_cookies_raw(tab) else {
        return Err("CDP getAllCookies 失败".to_string());
    };
    let found = cookies
        .as_array()
        .map(|arr| {
            arr.iter().any(|c| {
                c.get("name")
                    .and_then(|n| n.as_str())
                    .map(|name| cookie_names.iter().any(|target| target == name))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    if found {
        Ok(Some(LoginResult {
            logged_in: true,
            ..Default::default()
        }))
    } else {
        Ok(None)
    }
}

/// 执行 JS 脚本检测登录态（脚本须返回含 loggedIn 字段的 JSON）
fn eval_login_script(
    tab: &headless_chrome::Tab,
    script: &str,
    await_promise: bool,
) -> Result<Option<LoginResult>, String> {
    let result = tab
        .evaluate(script, await_promise)
        .map_err(|e| format!("{e}"))?;
    let parsed: Option<serde_json::Value> = result.value.as_ref().and_then(|v| {
        v.as_str()
            .and_then(|s| serde_json::from_str(s).ok())
            .or_else(|| Some(v.clone()))
    });

    match parsed {
        Some(info)
            if info
                .get("loggedIn")
                .and_then(|v| v.as_bool())
                .unwrap_or(false) =>
        {
            let mut lr = LoginResult {
                logged_in: true,
                ..Default::default()
            };
            merge_userinfo(&mut lr, &info);
            Ok(Some(lr))
        }
        _ => Ok(None),
    }
}

/// 从 JS 返回的 JSON 中提取用户信息字段到 LoginResult
fn extract_str<'a>(info: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    info.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

pub fn merge_userinfo(result: &mut LoginResult, info: &serde_json::Value) {
    if let Some(v) = extract_str(info, "userId") {
        result.user_id = v.to_string();
    }
    if let Some(v) = extract_str(info, "username") {
        result.username = v.to_string();
    }
    if let Some(v) = extract_str(info, "email") {
        result.email = v.to_string();
    }
    if let Some(v) = extract_str(info, "avatarUrl") {
        result.avatar_url = v.to_string();
    }
    if let Some(v) = extract_str(info, "tier") {
        result.tier = v.to_string();
    }
    if let Some(v) = info.get("credits").and_then(|v| v.as_i64()) {
        result.credits = v as i32;
    }
}

/// 修复 Chrome exit_type（供 browser_engine 等外部模块调用）
pub fn fix_chrome_exit_type_pub(profile_dir: &std::path::Path) {
    fix_chrome_exit_type(profile_dir);
}

fn fix_chrome_exit_type(profile_dir: &std::path::Path) {
    fix_prefs_json(profile_dir);
    cleanup_crash_artifacts(profile_dir);
}

fn fix_prefs_json(profile_dir: &std::path::Path) {
    let prefs_path = profile_dir.join("Default").join("Preferences");
    if !prefs_path.exists() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(&prefs_path) else {
        return;
    };
    let Ok(mut prefs) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };

    let mut changed = false;
    changed |= fix_profile_exit_type(&mut prefs);
    changed |= clear_session_restore(&mut prefs);
    changed |= ensure_welcome_page_seen(&mut prefs);

    if changed {
        if let Ok(json) = serde_json::to_string(&prefs) {
            let _ = std::fs::write(&prefs_path, json);
            tracing::debug!("已修复 Chrome exit_type + 清除 crash recovery 标记");
        }
    }
}

fn fix_profile_exit_type(prefs: &mut serde_json::Value) -> bool {
    let Some(profile) = prefs.get_mut("profile").and_then(|v| v.as_object_mut()) else {
        return false;
    };
    let mut changed = false;
    if profile.get("exit_type").and_then(|v| v.as_str()) != Some("Normal") {
        profile.insert("exit_type".to_string(), serde_json::json!("Normal"));
        changed = true;
    }
    if profile.get("exited_cleanly").and_then(|v| v.as_bool()) != Some(true) {
        profile.insert("exited_cleanly".to_string(), serde_json::json!(true));
        changed = true;
    }
    changed
}

fn clear_session_restore(prefs: &mut serde_json::Value) -> bool {
    prefs
        .get_mut("session")
        .and_then(|v| v.as_object_mut())
        .and_then(|s| s.remove("restore_on_startup"))
        .is_some()
}

fn ensure_welcome_page_seen(prefs: &mut serde_json::Value) -> bool {
    let Some(browser) = prefs.get_mut("browser").and_then(|v| v.as_object_mut()) else {
        return false;
    };
    if browser
        .get("has_seen_welcome_page")
        .and_then(|v| v.as_bool())
        != Some(true)
    {
        browser.insert("has_seen_welcome_page".to_string(), serde_json::json!(true));
        return true;
    }
    false
}

fn cleanup_crash_artifacts(profile_dir: &std::path::Path) {
    let crashpad = profile_dir.join("Crashpad");
    if crashpad.exists() {
        let _ = std::fs::remove_dir_all(&crashpad);
        tracing::debug!("已清除 Crashpad 目录");
    }
    let ss_dir = profile_dir.join("Default").join("Session Storage");
    if !ss_dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(&ss_dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains("LOCK") || name.contains("LOG") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
#[path = "browser_manager_tests.rs"]
mod browser_manager_tests;
