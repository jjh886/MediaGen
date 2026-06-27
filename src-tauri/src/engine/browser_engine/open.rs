use std::path::PathBuf;
use std::sync::Arc;

use headless_chrome::Browser;
use serde_json::json;
use tracing::info;

use super::cookie::{inject_cookies_via_cdp, prepare_user_data_dir, write_cookies_to_profile};
use super::open_process::spawn_chrome_process;
use super::{inject_stealth, kill_chrome_by_profile, BrowserSession, SharedBrowser};
use crate::engine::retry::RetryConfig;
use crate::error::AppError;
use crate::sync_ext::MutexExt;
use crate::types::StepResult;

impl super::BrowserEngine {
    /// browser.open: 启动 Chrome 浏览器实例（带 stealth 补丁 + 可选 cookie 恢复）
    ///
    /// `force_restart: true` 强制清理该 profile 的所有 session/browser 条目并 kill 进程，
    /// 跳过复用检测，专供"rate limited 后启动有头浏览器人工验证"等场景使用。
    ///
    /// `offscreen: true` 启动时追加 `--window-position=-32000,-32000`，窗口移出屏幕，
    /// 用户看不到；后续可通过 `browser.show_window` 把窗口移回可见区域。
    pub(crate) async fn open(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let headless = params["headless"].as_bool().unwrap_or(false);
        let force_restart = params["force_restart"].as_bool().unwrap_or(false);
        let offscreen = params["offscreen"].as_bool().unwrap_or(false);
        let profile_dir = params["profile_dir"].as_str().unwrap_or("");
        let chrome_path = params["chrome_path"].as_str();
        let session_id = params["session_id"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let cookies_json = params["cookies_json"].as_str().unwrap_or("");
        let restore_url = params["restore_url"].as_str().unwrap_or("");
        let retry = RetryConfig::from_params(params, 2, &[3_000, 8_000]);

        let profile_dir_str = profile_dir.to_string();

        if force_restart {
            tracing::warn!(
                profile_dir = %profile_dir_str,
                "force_restart=true，强制清理 profile 所有 session/browser 条目并 kill 进程"
            );
            self.force_release_profile(&profile_dir_str);
        }

        // 尝试复用已有 session 或 browser
        if let Some(result) = self
            .try_reuse_session(&session_id, &profile_dir_str, headless)
            .await
        {
            return result;
        }

        // 等待同 profile 的并发启动
        if let Some(result) = self
            .wait_concurrent_launch(&session_id, &profile_dir_str, headless)
            .await
        {
            return result;
        }

        tracing::warn!(
            session_id = %session_id,
            profile_dir = %profile_dir,
            "未找到可复用的 session 或 browser，启动新 Chrome 实例"
        );

        if !profile_dir_str.is_empty() {
            kill_chrome_by_profile(&profile_dir_str);
        }

        let result = self
            .launch_new_chrome(
                &session_id,
                &profile_dir_str,
                headless,
                offscreen,
                chrome_path,
                cookies_json,
                restore_url,
                retry,
            )
            .await;

        self.launching.lock_recover().remove(&profile_dir_str);
        result
    }

    async fn try_reuse_session(
        &self,
        session_id: &str,
        profile_dir_str: &str,
        headless: bool,
    ) -> Option<Result<StepResult, AppError>> {
        self.try_reuse_existing_session(session_id, profile_dir_str, headless)
            .or_else(|| self.try_reuse_profile_browser(session_id, profile_dir_str, headless))
    }

    fn try_reuse_existing_session(
        &self,
        session_id: &str,
        profile_dir_str: &str,
        headless: bool,
    ) -> Option<Result<StepResult, AppError>> {
        let mut sessions = self.sessions.lock_recover();
        let session = sessions.get_mut(session_id)?;
        let alive = session.tab.evaluate("1+1", false).is_ok();
        info!(session_id = %session_id, alive, in_use = session.in_use, "检测已有 session");

        if alive {
            session.in_use = true;
            return Some(Ok(reused_open_result(session_id, headless, false)));
        }

        let profile = session.profile_dir.clone();
        sessions.remove(session_id);
        drop(sessions);
        self.try_open_tab_from_browser(session_id, &profile, profile_dir_str, headless, false)
    }

    fn try_reuse_profile_browser(
        &self,
        session_id: &str,
        profile_dir_str: &str,
        headless: bool,
    ) -> Option<Result<StepResult, AppError>> {
        if profile_dir_str.is_empty() {
            return None;
        }
        self.try_open_tab_from_browser(session_id, profile_dir_str, profile_dir_str, headless, true)
    }

    fn try_open_tab_from_browser(
        &self,
        session_id: &str,
        profile_key: &str,
        stealth_profile: &str,
        headless: bool,
        increment_ref: bool,
    ) -> Option<Result<StepResult, AppError>> {
        let browsers = self.browsers.lock_recover();
        let browser = browsers.get(profile_key)?;
        match browser.browser.new_tab() {
            Ok(new_tab) => {
                inject_stealth(&new_tab, stealth_profile);
                drop(browsers);
                self.register_reused_tab(session_id, profile_key, new_tab);
                if increment_ref {
                    self.increment_browser_ref(profile_key);
                }
                Some(Ok(reused_open_result(session_id, headless, true)))
            }
            Err(e) => {
                info!(error = %e, "browser 已失效，清理后重启");
                drop(browsers);
                self.remove_stale_browser(profile_key);
                None
            }
        }
    }

    fn register_reused_tab(
        &self,
        session_id: &str,
        profile_key: &str,
        tab: Arc<headless_chrome::Tab>,
    ) {
        // 复用 Chrome 时只新建 tab，不重启浏览器，避免不必要的登录态丢失。
        let mut sessions = self.sessions.lock_recover();
        sessions.insert(
            session_id.to_string(),
            BrowserSession {
                tab,
                profile_dir: profile_key.to_string(),
                in_use: true,
            },
        );
    }

    fn increment_browser_ref(&self, profile_key: &str) {
        let mut browsers = self.browsers.lock_recover();
        if let Some(browser) = browsers.get_mut(profile_key) {
            browser.ref_count += 1;
        }
    }

    fn remove_stale_browser(&self, profile_key: &str) {
        let mut browsers = self.browsers.lock_recover();
        browsers.remove(profile_key);
        drop(browsers);
        kill_chrome_by_profile(profile_key);
    }

    async fn wait_concurrent_launch(
        &self,
        session_id: &str,
        profile_dir_str: &str,
        headless: bool,
    ) -> Option<Result<StepResult, AppError>> {
        if profile_dir_str.is_empty() {
            return None;
        }

        loop {
            {
                let launching = self.launching.lock_recover();
                if !launching.contains(profile_dir_str) {
                    break;
                }
            }
            tracing::debug!(profile_dir = %profile_dir_str, "等待同 profile 的 Chrome 启动完成");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let browsers = self.browsers.lock_recover();
            if let Some(sb) = browsers.get(profile_dir_str) {
                match sb.browser.new_tab() {
                    Ok(new_tab) => {
                        inject_stealth(&new_tab, profile_dir_str);
                        drop(browsers);
                        {
                            let mut browsers = self.browsers.lock_recover();
                            if let Some(sb) = browsers.get_mut(profile_dir_str) {
                                sb.ref_count += 1;
                            }
                        }
                        let mut sessions = self.sessions.lock_recover();
                        sessions.insert(
                            session_id.to_string(),
                            BrowserSession {
                                tab: new_tab,
                                profile_dir: profile_dir_str.to_string(),
                                in_use: true,
                            },
                        );
                        return Some(Ok(StepResult {
                            success: true,
                            data: json!({ "session_id": session_id, "headless": headless, "reused": true, "new_tab": true, "cookies_restored": 0 }),
                            error: None,
                        }));
                    }
                    Err(_) => {}
                }
            }
        }

        self.launching
            .lock_recover()
            .insert(profile_dir_str.to_string());
        None
    }

    async fn launch_new_chrome(
        &self,
        session_id: &str,
        profile_dir_str: &str,
        headless: bool,
        offscreen: bool,
        chrome_path: Option<&str>,
        cookies_json: &str,
        _restore_url: &str,
        retry: RetryConfig,
    ) -> Result<StepResult, AppError> {
        let user_data_dir = prepare_user_data_dir(profile_dir_str);

        // macOS/Linux: 启动前写入 profile SQLite，Chrome 能直接读取明文 cookie
        // Windows: 跳过 SQLite 预写（Windows Chrome 使用 DPAPI 加密，明文 value 不被读取），
        //          改为启动后通过 CDP 注入
        let cookies_written = if !cfg!(target_os = "windows")
            && !profile_dir_str.is_empty()
            && !cookies_json.is_empty()
        {
            write_cookies_to_profile(profile_dir_str, cookies_json)
        } else {
            0
        };

        let chrome_pb = chrome_path.filter(|p| !p.is_empty()).map(PathBuf::from);
        let session_id = session_id.to_string();
        let profile_dir_str = profile_dir_str.to_string();
        let profile_dir_for_key = profile_dir_str.clone();

        let (browser, tab) = spawn_chrome_process(
            chrome_pb,
            user_data_dir,
            headless,
            offscreen,
            profile_dir_str,
            retry,
        )
        .await?;

        // Windows: Chrome 启动后通过 CDP Network.setCookie 注入 cookie
        let cdp_restored = if cfg!(target_os = "windows") && !cookies_json.is_empty() {
            inject_cookies_via_cdp(&tab, cookies_json).await
        } else {
            0
        };

        let total_restored = cookies_written + cdp_restored;
        info!(session_id = %session_id, headless, cookies_written, cdp_restored, "浏览器已启动");

        self.register_session(&session_id, &profile_dir_for_key, browser, tab);

        Ok(StepResult {
            success: true,
            data: json!({ "session_id": session_id, "headless": headless, "cookies_restored": total_restored }),
            error: None,
        })
    }

    /// 把 BrowserEngine 外部已经创建的 `(browser, tab)` 登记到内部映射里，
    /// 后续 `browser.open` 按 `profile_dir` 复用时能命中这个实例。
    ///
    /// 场景：登录流程常驻模式下，Chrome 由 `browser_manager::open_and_wait_for_login`
    /// 启动，登录成功后不关闭，直接交给 BrowserEngine 继续使用。
    ///
    /// 返回生成的 `session_id`。新 session 的 `in_use = false`，首次 `browser.open`
    /// 时会被复用路径抢占。
    pub fn register_external_browser(
        &self,
        profile_dir: &str,
        browser: headless_chrome::Browser,
        tab: std::sync::Arc<headless_chrome::Tab>,
    ) -> String {
        let session_id = uuid::Uuid::new_v4().to_string();
        let browser_key = if profile_dir.is_empty() {
            session_id.clone()
        } else {
            profile_dir.to_string()
        };
        {
            let mut browsers = self.browsers.lock_recover();
            browsers.insert(
                browser_key.clone(),
                SharedBrowser {
                    browser,
                    ref_count: 1,
                },
            );
        }
        {
            let mut sessions = self.sessions.lock_recover();
            sessions.insert(
                session_id.clone(),
                BrowserSession {
                    tab,
                    profile_dir: browser_key,
                    // 登录完成后 Chrome 处于空闲状态，后续任务的 browser.open 会抢占此 session
                    in_use: false,
                },
            );
        }
        tracing::info!(session_id = %session_id, profile_dir, "已登记 persistent 浏览器到 engine");
        session_id
    }

    /// 关闭 engine 内所有被 persistent 模式保留下来的浏览器实例。
    /// 用于 App 退出时统一清理，确保 Chrome 进程不残留。
    pub fn shutdown_all_persistent(&self) {
        let profile_dirs: Vec<String> = {
            let browsers = self.browsers.lock_recover();
            browsers.keys().cloned().collect()
        };
        for profile_dir in profile_dirs {
            self.force_release_profile(&profile_dir);
        }
    }

    /// 强制释放指定 profile 的所有 session/browser 条目并 kill 残留 Chrome 进程。
    /// 供 `force_restart` 参数使用，确保从 headless 切换到有头时不会复用旧实例。
    pub fn force_release_profile(&self, profile_dir: &str) {
        if profile_dir.is_empty() {
            return;
        }
        let session_ids: Vec<String> = {
            let sessions = self.sessions.lock_recover();
            sessions
                .iter()
                .filter(|(_, s)| s.profile_dir == profile_dir)
                .map(|(id, _)| id.clone())
                .collect()
        };
        {
            let mut sessions = self.sessions.lock_recover();
            for sid in &session_ids {
                sessions.remove(sid);
            }
        }
        {
            let mut browsers = self.browsers.lock_recover();
            browsers.remove(profile_dir);
        }
        {
            let mut launching = self.launching.lock_recover();
            launching.remove(profile_dir);
        }
        kill_chrome_by_profile(profile_dir);
        tracing::info!(
            profile_dir,
            cleared_sessions = session_ids.len(),
            "force_release_profile 完成"
        );
    }
}

fn reused_open_result(session_id: &str, headless: bool, new_tab: bool) -> StepResult {
    let data = if new_tab {
        json!({ "session_id": session_id, "headless": headless, "reused": true, "new_tab": true, "cookies_restored": 0 })
    } else {
        json!({ "session_id": session_id, "headless": headless, "reused": true, "cookies_restored": 0 })
    };
    StepResult {
        success: true,
        data,
        error: None,
    }
}

impl super::BrowserEngine {
    /// 注册 browser 和 session 到内部状态
    fn register_session(
        &self,
        session_id: &str,
        profile_dir: &str,
        browser: Browser,
        tab: std::sync::Arc<headless_chrome::Tab>,
    ) {
        let browser_key = if profile_dir.is_empty() {
            session_id.to_string()
        } else {
            profile_dir.to_string()
        };
        {
            let mut browsers = self.browsers.lock_recover();
            browsers.insert(
                browser_key,
                SharedBrowser {
                    browser,
                    ref_count: 1,
                },
            );
        }
        let effective_profile = if profile_dir.is_empty() {
            session_id.to_string()
        } else {
            profile_dir.to_string()
        };
        let mut sessions = self.sessions.lock_recover();
        sessions.insert(
            session_id.to_string(),
            BrowserSession {
                tab,
                profile_dir: effective_profile,
                in_use: true,
            },
        );
    }
}
