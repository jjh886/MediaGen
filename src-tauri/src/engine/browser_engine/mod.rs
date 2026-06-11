mod close;
mod cookie;
mod external;
mod file_input;
mod interaction;
mod navigation;
mod open;
mod open_process;
mod process;
mod session;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use headless_chrome::Browser;

use crate::engine::condition::evaluate_condition;
use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::sync_ext::MutexExt;
use crate::types::StepResult;

pub(crate) use process::{
    close_tabs_with_unload, graceful_shutdown_owned_browser, graceful_terminate_and_wait,
    kill_chrome_by_profile,
};

pub struct BrowserEngine {
    pub(crate) sessions: Mutex<HashMap<String, BrowserSession>>,
    pub(crate) browsers: Mutex<HashMap<String, SharedBrowser>>,
    pub(crate) launching: Mutex<std::collections::HashSet<String>>,
}

pub(crate) struct BrowserSession {
    pub(crate) tab: Arc<headless_chrome::Tab>,
    pub(crate) profile_dir: String,
    pub(crate) in_use: bool,
}

pub(crate) struct SharedBrowser {
    pub(crate) browser: Browser,
    pub(crate) ref_count: usize,
}

impl BrowserEngine {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            browsers: Mutex::new(HashMap::new()),
            launching: Mutex::new(std::collections::HashSet::new()),
        }
    }

    pub(crate) fn get_tab(&self, session_id: &str) -> Result<Arc<headless_chrome::Tab>, AppError> {
        let sessions = self.sessions.lock_recover();
        sessions
            .get(session_id)
            .map(|s| s.tab.clone())
            .ok_or_else(|| AppError::BrowserError(format!("浏览器会话不存在: {session_id}")))
    }
}

/// 把 `Arc<BrowserEngine>` 套一层，让 EngineRegistry 按 `Box<dyn AtomExecutor>` 注册的
/// 同时，AppState 仍可持有同一个 `Arc<BrowserEngine>` 直接调用具体方法
/// （如 `register_external_browser`、`shutdown_all_persistent`）。
pub struct SharedBrowserEngine {
    inner: Arc<BrowserEngine>,
}

impl From<Arc<BrowserEngine>> for SharedBrowserEngine {
    fn from(inner: Arc<BrowserEngine>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl AtomExecutor for SharedBrowserEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        self.inner.handles(action_prefix)
    }

    async fn execute(
        &self,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        self.inner.execute(action, params).await
    }
}

#[async_trait]
impl AtomExecutor for BrowserEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "browser"
    }

    async fn execute(
        &self,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        match action {
            "browser.open" => self.open(params).await,
            "browser.navigate" => self.navigate(params).await,
            "browser.run_script" => self.run_script(params).await,
            "browser.get_cookies" => self.get_cookies(params).await,
            "browser.click" => self.click_element(params).await,
            "browser.click_text" => self.click_text(params).await,
            "browser.type_text" => self.type_text(params).await,
            "browser.press_key" => self.press_key(params).await,
            "browser.set_file" => self.set_file(params).await,
            "browser.download" => self.browser_download(params).await,
            "browser.restore_cookies" => self.restore_cookies(params).await,
            "browser.close" => self.close(params).await,
            "browser.launch_external" => self.launch_external(params).await,
            "browser.poll_script" => self.poll_script(params).await,
            "browser.show_window" => self.show_window(params).await,
            _ => Err(AppError::PlatformError(format!(
                "未知的 browser 操作: {action}，可用: browser.open, browser.navigate, browser.run_script, browser.click, browser.type_text, browser.press_key, browser.get_cookies, browser.restore_cookies, browser.close, browser.launch_external, browser.poll_script, browser.show_window"
            ))),
        }
    }
}
/// 注入 stealth 补丁：隐藏自动化特征，降低被反爬系统检测的风险
///
/// 基于 profile_dir 生成每个账号独立的设备指纹，避免多账号共享同一指纹被检测
fn inject_stealth(tab: &headless_chrome::Tab, profile_dir: &str) {
    override_user_agent(tab);
    let fingerprint = AccountFingerprint::from_profile(profile_dir);
    inject_stealth_js(tab, &fingerprint);
}

/// 基于 profile_dir hash 生成的稳定、唯一的账号指纹参数
struct AccountFingerprint {
    hardware_concurrency: u8,
    device_memory: u8,
    screen_width: u16,
    screen_height: u16,
    color_depth: u8,
    max_touch_points: u8,
    languages_index: u8,
}

impl AccountFingerprint {
    fn from_profile(profile_dir: &str) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        profile_dir.hash(&mut hasher);
        let h = hasher.finish();

        let cores: [u8; 4] = [4, 8, 12, 16];
        let memory: [u8; 4] = [4, 8, 16, 32];
        let resolutions: [(u16, u16); 6] = [
            (1920, 1080),
            (2560, 1440),
            (1680, 1050),
            (1440, 900),
            (1536, 864),
            (1366, 768),
        ];
        let depths: [u8; 2] = [24, 30];

        let (sw, sh) = resolutions[(h as usize) % resolutions.len()];
        Self {
            hardware_concurrency: cores[((h >> 8) as usize) % cores.len()],
            device_memory: memory[((h >> 16) as usize) % memory.len()],
            screen_width: sw,
            screen_height: sh,
            color_depth: depths[((h >> 24) as usize) % depths.len()],
            max_touch_points: 0,
            languages_index: ((h >> 32) as u8) % 4,
        }
    }

    /// 输出一个 JS 字面量对象，供 stealth 闭包内通过 defineGetter 安装到 navigator/screen，
    /// 这样所有指纹 getter 都能加入 _patchedFns，通过 .toString() native-code 检测。
    fn to_js_literal(&self) -> String {
        let languages = match self.languages_index {
            0 => "['zh-CN', 'zh', 'en']",
            1 => "['zh-CN', 'en', 'zh']",
            2 => "['zh-CN', 'zh', 'en-US', 'en']",
            _ => "['zh-CN', 'en-US']",
        };
        format!(
            "{{ hc: {hc}, dm: {dm}, sw: {sw}, sh: {sh}, cd: {cd}, tp: {tp}, langs: {langs} }}",
            hc = self.hardware_concurrency,
            dm = self.device_memory,
            sw = self.screen_width,
            sh = self.screen_height,
            cd = self.color_depth,
            tp = self.max_touch_points,
            langs = languages,
        )
    }
}

/// 覆盖 User-Agent，去掉 HeadlessChrome 标识
fn override_user_agent(tab: &headless_chrome::Tab) {
    let Ok(ua) = tab.evaluate("navigator.userAgent", false) else {
        return;
    };
    let Some(ua_str) = ua.value.and_then(|v| v.as_str().map(String::from)) else {
        return;
    };
    let clean_ua = ua_str.replace("HeadlessChrome", "Chrome");
    let platform_hint = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    };
    if let Err(e) = tab.set_user_agent(&clean_ua, Some("zh-CN"), Some(platform_hint)) {
        tracing::warn!(error = %e, "UA 覆盖失败");
    }
}

/// stealth JS 基础脚本（不含指纹参数，指纹由 AccountFingerprint 动态生成）
///
/// 关键点：
/// - 所有被覆盖的 getter / 改写的 function 都必须进入 _patchedFns，否则 .toString()
///   会暴露非 [native code] 特征
/// - 所有新增对象伪装（WebGL / userAgentData / chrome）都必须对应到 getter 伪装
/// - 使用 Proxy 让 Function.prototype.toString 对自身也返回 native code
///
/// 脚本内容在同目录的 stealth.js.tmpl，编译期通过 include_str! 内联；
/// 放在独立文件里而非 Rust 字面量，是为了让结构复杂度门禁不把 JS 误判为 Rust 函数。
const STEALTH_JS_TEMPLATE: &str = include_str!("stealth.js.tmpl");

/// 通过 CDP 持久化注入 stealth JS + 账号指纹，失败则降级为一次性注入
fn inject_stealth_js(tab: &headless_chrome::Tab, fingerprint: &AccountFingerprint) {
    let stealth_js = STEALTH_JS_TEMPLATE.replace("__FINGERPRINT__", &fingerprint.to_js_literal());
    use headless_chrome::protocol::cdp::Page;
    if let Err(e) = tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: stealth_js.clone(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    }) {
        tracing::warn!(error = %e, "stealth 持久化注入失败，降级为一次性注入");
        if let Err(e2) = tab.evaluate(&stealth_js, false) {
            tracing::warn!(error = %e2, "stealth 一次性注入也失败");
        }
    }
    // 当前页面也立即执行一次
    let _ = tab.evaluate(&stealth_js, false);
    tracing::debug!("stealth 补丁 + 账号指纹已注入");
}

/// 简单条件求值（复用 http_engine 同样的逻辑）
fn evaluate_poll_condition(data: &serde_json::Value, condition: &str) -> bool {
    evaluate_condition(data, condition)
}

/// 通过原始 CDP `Network.getAllCookies` 拉所有 cookies，返回 `cookies` 字段原值。
///
/// 为什么不直接用 `tab.get_cookies()`：headless_chrome 把 CDP `Network.Cookie.sourcePort`
/// 生成为 `u32`，但协议规定端口未知时返回 `-1`。只要有一条 cookie 的 sourcePort=-1
/// （部分网页登录态 cookie 常见），`get_cookies()` 直接返回
/// `invalid value integer -1, expected u32` 错误。这里用 Value 接住，绕开强类型。
///
/// 登录检测 / cookie 备份 / 登录态轮询都复用它，避免三处各自处理。
pub(crate) fn get_all_cookies_raw(tab: &headless_chrome::Tab) -> Option<serde_json::Value> {
    #[derive(Debug, serde::Serialize)]
    struct GetAllCookiesRaw;
    impl headless_chrome::protocol::cdp::types::Method for GetAllCookiesRaw {
        const NAME: &'static str = "Network.getAllCookies";
        type ReturnObject = serde_json::Value;
    }
    match tab.call_method(GetAllCookiesRaw) {
        Ok(value) => value.get("cookies").cloned(),
        Err(e) => {
            tracing::debug!(error = %e, "CDP getAllCookies 失败");
            None
        }
    }
}

/// 备份 tab 的全部 cookies 为 JSON 字符串；失败返回空串。基于 [`get_all_cookies_raw`]。
pub(crate) fn dump_cookies_as_json(tab: &headless_chrome::Tab) -> String {
    match get_all_cookies_raw(tab) {
        Some(arr) => {
            let count = arr.as_array().map(|a| a.len()).unwrap_or(0);
            tracing::debug!(count, "备份 cookie");
            serde_json::to_string(&arr).unwrap_or_default()
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_prefix() {
        let engine = BrowserEngine::new();
        assert!(engine.handles("browser"));
        assert!(!engine.handles("http"));
    }

    #[test]
    fn test_poll_condition() {
        let data = serde_json::json!({"status": "done", "url": "https://example.com/img.png"});
        assert!(evaluate_poll_condition(&data, "status == 'done'"));
        assert!(!evaluate_poll_condition(&data, "status == 'pending'"));
        assert!(evaluate_poll_condition(&data, "status != 'pending'"));
    }
}
