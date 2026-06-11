//! `browser.close` 的全部实现：释放/关闭浏览器，含并发安全语义。
//!
//! 独立成文件是为了把 session 级"交互操作"（get_cookies / download / restore_cookies
//! / show_window）和"生命周期结束"（close + 其配套 cookies 备份 / 优雅退出）两类职责分开。
//!
//! 测试拆到同目录 `tests.rs`，单文件满足 500 行上限约束。

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;
use tracing::info;

use super::BrowserSession;
use crate::error::AppError;
use crate::sync_ext::MutexExt;
use crate::types::StepResult;

#[cfg(test)]
mod tests;

/// 统计隶属指定 profile 的 session 数量。放在模块级以便单测直接喂 HashMap，
/// 避免为计数逻辑启动真实 Chrome。
pub(super) fn count_sessions_with_profile(
    sessions: &HashMap<String, BrowserSession>,
    profile_dir: &str,
) -> usize {
    sessions
        .values()
        .filter(|s| s.profile_dir == profile_dir)
        .count()
}

/// 统计"同 profile 下、除 exclude_session_id 外、in_use=true"的 session 数量。
/// 用元组迭代器形式，单测可直接构造样本（BrowserSession 含真实 Tab，无法 mock）。
pub(super) fn count_other_active_sessions<'a, I>(
    entries: I,
    exclude_session_id: &str,
    profile_dir: &str,
) -> usize
where
    I: Iterator<Item = (&'a str, &'a str, bool)>, // (session_id, profile_dir, in_use)
{
    entries
        .filter(|(sid, pd, in_use)| *sid != exclude_session_id && *pd == profile_dir && *in_use)
        .count()
}

impl super::BrowserEngine {
    /// browser.close: 释放/关闭浏览器
    ///
    /// 三种调用方式：
    /// - `session_id` only：原语义。默认仅标记空闲保留常驻；`force=true` 时关闭 tab 并按
    ///   引用计数决定是否回收 browser。Flow 等 persistent 平台使用。
    /// - `profile_dir` only：兜底清理路径（`post_flow_cleanup` 的 `force_close_browser`）。
    ///   有并发任务时跳过，避免误杀其他 tab 的 CDP 连接。
    /// - `session_id + profile_dir`（PerTask 并发安全）：先释放自己的 tab，若同 profile 下
    ///   还有其他活跃任务则保留 Chrome；若自己是最后一个则 graceful 关闭 Chrome。
    ///   适用于同一 profile 下存在并发任务的场景。
    pub(crate) async fn close(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let session_id = params["session_id"].as_str().unwrap_or("").to_string();
        let profile_dir = params["profile_dir"].as_str().unwrap_or("").to_string();
        let force = params["force"].as_bool().unwrap_or(false);

        if session_id.is_empty() && profile_dir.is_empty() {
            return Err(AppError::BrowserError(
                "browser.close 需要 session_id 或 profile_dir".into(),
            ));
        }

        if session_id.is_empty() && force {
            let cookies_json = self.backup_cookies_for_profile(&profile_dir).await;
            self.force_release_profile(&profile_dir);
            return Ok(StepResult {
                success: true,
                data: json!({
                    "profile_dir": profile_dir,
                    "closed": true,
                    "released": false,
                    "cookies_json": cookies_json,
                    "forced": true,
                }),
                error: None,
            });
        }

        if session_id.is_empty() {
            return self.close_by_profile_only(&profile_dir).await;
        }

        if !profile_dir.is_empty() && !force {
            return self
                .close_session_scoped_to_profile(&session_id, &profile_dir)
                .await;
        }

        self.close_session_legacy(&session_id, force).await
    }

    /// session_id-only 分支：保留既有语义（默认只释放，force=true 才关闭 tab）。
    /// Flow persistent 平台或显式 `force=true` 时走这条路径。
    async fn close_session_legacy(
        &self,
        session_id: &str,
        force: bool,
    ) -> Result<StepResult, AppError> {
        let cookies_json = self.backup_cookies_for_session(session_id).await;
        let mut sessions = self.sessions.lock_recover();

        if force {
            self.close_tab_and_decrement(&mut sessions, session_id);
        } else {
            self.release_or_close_tab_persistent(&mut sessions, session_id);
        }

        Ok(StepResult {
            success: true,
            data: json!({
                "session_id": session_id,
                "closed": force,
                "released": !force,
                "cookies_json": cookies_json,
            }),
            error: None,
        })
    }

    /// force=true：关 tab、减 ref_count，ref_count=0 时 remove browser。
    fn close_tab_and_decrement(
        &self,
        sessions: &mut HashMap<String, BrowserSession>,
        session_id: &str,
    ) {
        let Some(removed) = sessions.remove(session_id) else {
            return;
        };
        let _ = removed.tab.close(true);

        if removed.profile_dir.is_empty() {
            info!(session_id, "浏览器会话已关闭（force）");
            return;
        }
        let mut browsers = self.browsers.lock_recover();
        let Some(sb) = browsers.get_mut(&removed.profile_dir) else {
            return;
        };
        sb.ref_count = sb.ref_count.saturating_sub(1);
        if sb.ref_count == 0 {
            browsers.remove(&removed.profile_dir);
            info!(session_id, "浏览器实例已关闭（最后一个 session）");
        } else {
            info!(
                session_id,
                ref_count = sb.ref_count,
                "tab 已关闭，browser 仍有其他 session"
            );
        }
    }

    /// force=false 常驻模式：同 profile 多 tab 时安全关 tab；唯一 tab 仅标空闲（Chrome 最后
    /// 一个 tab 关闭会导致进程退出，persistent 平台不能走这条）。
    fn release_or_close_tab_persistent(
        &self,
        sessions: &mut HashMap<String, BrowserSession>,
        session_id: &str,
    ) {
        let profile_of_current = sessions.get(session_id).map(|s| s.profile_dir.clone());
        let same_profile_count = profile_of_current
            .as_deref()
            .map(|pd| count_sessions_with_profile(sessions, pd))
            .unwrap_or(0);

        if same_profile_count > 1 {
            let Some(removed) = sessions.remove(session_id) else {
                return;
            };
            let _ = removed.tab.close(true);
            if removed.profile_dir.is_empty() {
                return;
            }
            let mut browsers = self.browsers.lock_recover();
            if let Some(sb) = browsers.get_mut(&removed.profile_dir) {
                sb.ref_count = sb.ref_count.saturating_sub(1);
                info!(
                    session_id,
                    ref_count = sb.ref_count,
                    "常驻模式：临时 tab 已关闭，browser 保活"
                );
            }
        } else if let Some(session) = sessions.get_mut(session_id) {
            session.in_use = false;
            info!(session_id, "浏览器会话已释放（profile 唯一 tab，保持常驻）");
        }
    }

    /// profile_dir-only 分支：并发 guard 版本。
    /// profile 下仍有 in_use session 时跳过 `gracefully_close_profile`，避免并发场景
    /// 下 `post_flow_cleanup` 的兜底调用 SIGTERM 掉其他任务共用的 Chrome 进程。
    async fn close_by_profile_only(&self, profile_dir: &str) -> Result<StepResult, AppError> {
        let others_active = self.count_other_active_in_profile("", profile_dir);
        let cookies_json = self.backup_cookies_for_profile(profile_dir).await;

        if others_active > 0 {
            tracing::info!(
                profile_dir,
                others_active,
                "profile 下仍有活跃任务，跳过 Chrome 关闭（仅备份 cookies）"
            );
            return Ok(StepResult {
                success: true,
                data: json!({
                    "profile_dir": profile_dir,
                    "closed": false,
                    "released": false,
                    "cookies_json": cookies_json,
                    "skipped_reason": "other_sessions_active",
                }),
                error: None,
            });
        }

        self.gracefully_close_profile(profile_dir).await;
        Ok(StepResult {
            success: true,
            data: json!({
                "profile_dir": profile_dir,
                "closed": true,
                "released": false,
                "cookies_json": cookies_json,
            }),
            error: None,
        })
    }

    /// session_id + profile_dir 分支：PerTask 并发安全语义。
    /// - 同 profile 下还有其他活跃 session → 只关自己的 tab，Chrome 保留
    /// - 自己是最后一个 → `gracefully_close_profile`（SIGTERM + 等待 + Drop 兜底）
    async fn close_session_scoped_to_profile(
        &self,
        session_id: &str,
        profile_dir: &str,
    ) -> Result<StepResult, AppError> {
        let cookies_json = self.backup_cookies_for_session(session_id).await;
        let others_active = self.count_other_active_in_profile(session_id, profile_dir);

        if others_active > 0 {
            self.close_own_tab_keep_browser(session_id, profile_dir)
                .await;
            tracing::info!(
                session_id,
                profile_dir,
                others_active,
                "并发场景：关闭当前 tab，保留 Chrome 进程"
            );
            return Ok(StepResult {
                success: true,
                data: json!({
                    "session_id": session_id,
                    "profile_dir": profile_dir,
                    "closed": true,
                    "released": false,
                    "cookies_json": cookies_json,
                    "chrome_kept": true,
                }),
                error: None,
            });
        }

        self.gracefully_close_profile(profile_dir).await;
        Ok(StepResult {
            success: true,
            data: json!({
                "session_id": session_id,
                "profile_dir": profile_dir,
                "closed": true,
                "released": false,
                "cookies_json": cookies_json,
                "chrome_kept": false,
            }),
            error: None,
        })
    }

    /// 关闭指定 session 的 tab 并减少 browser 引用计数，保留 Chrome 进程。
    /// `tab.close` 是同步阻塞 CDP 调用，必须放到 `spawn_blocking` 且在锁外，
    /// 避免持 `sessions` 锁阻塞其他请求（这是之前"接口不响应"的放大因素）。
    async fn close_own_tab_keep_browser(&self, session_id: &str, profile_dir: &str) {
        let removed_tab = {
            let mut sessions = self.sessions.lock_recover();
            sessions.remove(session_id).map(|s| s.tab)
        };
        let Some(tab) = removed_tab else { return };
        let _ = tokio::task::spawn_blocking(move || tab.close(true)).await;

        let mut browsers = self.browsers.lock_recover();
        if let Some(sb) = browsers.get_mut(profile_dir) {
            sb.ref_count = sb.ref_count.saturating_sub(1);
            info!(
                session_id,
                ref_count = sb.ref_count,
                "并发场景：当前 tab 已关闭，browser 保留"
            );
        }
    }

    /// 计算同 profile 下除 exclude_session_id 外的 in_use session 数量。
    fn count_other_active_in_profile(&self, exclude_session_id: &str, profile_dir: &str) -> usize {
        let sessions = self.sessions.lock_recover();
        count_other_active_sessions(
            sessions
                .iter()
                .map(|(id, s)| (id.as_str(), s.profile_dir.as_str(), s.in_use)),
            exclude_session_id,
            profile_dir,
        )
    }

    /// 优雅关闭指定 profile 下的全部浏览器实例：
    /// 1. 对每个 tab 发 CDP `Page.Close`（执行 beforeunload 并关窗），让 Chrome 自己进入关闭流程
    /// 2. 取主进程 PID，发 SIGTERM（Unix）/ `taskkill` 不带 /F（Windows），等价于用户菜单里"退出"
    /// 3. 最多轮询 5 秒等 Chrome 自然退出；期间不阻塞 tokio runtime
    /// 4. 最后从映射中移除 session/browser：若进程已退出这是纯清理；若超时未退，`Browser::Drop`
    ///    会发最终 CDP Browser.Close + SIGKILL 作最后兜底——这是为满足"一定要关掉"的底线
    pub(super) async fn gracefully_close_profile(&self, profile_dir: &str) {
        if profile_dir.is_empty() {
            return;
        }

        let (pid, tabs, session_ids) = self.snapshot_profile_for_shutdown(profile_dir);

        // flow 里的 `browser.close` 已经清空过该 profile 时，post_flow_cleanup 的
        // 兜底再调一次会命中空 state。没必要再走 spawn_blocking + 打"关闭完成"日志，
        // 直接静默返回，避免重复日志。
        if pid.is_none() && tabs.is_empty() && session_ids.is_empty() {
            return;
        }

        let label = format!("任务结束 profile={profile_dir}");
        tokio::task::spawn_blocking(move || {
            super::close_tabs_with_unload(&tabs);
            if let Some(pid) = pid {
                super::graceful_terminate_and_wait(pid, std::time::Duration::from_secs(5), &label);
            }
        })
        .await
        .ok();

        {
            let mut sessions = self.sessions.lock_recover();
            for sid in &session_ids {
                sessions.remove(sid);
            }
        }
        self.browsers.lock_recover().remove(profile_dir);
        self.launching.lock_recover().remove(profile_dir);
        info!(
            profile_dir,
            cleared_sessions = session_ids.len(),
            "浏览器优雅关闭完成"
        );
    }

    /// 在短暂持锁期内快照 profile 下的 PID / 全部 tab / 全部 session_id，
    /// 后续阻塞操作都放到锁外以避免卡其他请求。
    fn snapshot_profile_for_shutdown(
        &self,
        profile_dir: &str,
    ) -> (Option<u32>, Vec<Arc<headless_chrome::Tab>>, Vec<String>) {
        let sessions = self.sessions.lock_recover();
        let browsers = self.browsers.lock_recover();
        let pid = browsers
            .get(profile_dir)
            .and_then(|sb| sb.browser.get_process_id());
        let mut tabs = Vec::new();
        let mut ids = Vec::new();
        for (sid, s) in sessions.iter() {
            if s.profile_dir == profile_dir {
                tabs.push(s.tab.clone());
                ids.push(sid.clone());
            }
        }
        (pid, tabs, ids)
    }

    pub(super) async fn backup_cookies_for_session(&self, session_id: &str) -> String {
        let Some(tab) = self.get_tab(session_id).ok() else {
            return String::new();
        };
        tokio::task::spawn_blocking(move || super::dump_cookies_as_json(&tab))
            .await
            .unwrap_or_default()
    }

    pub(super) async fn backup_cookies_for_profile(&self, profile_dir: &str) -> String {
        if profile_dir.is_empty() {
            return String::new();
        }
        let tab = {
            let sessions = self.sessions.lock_recover();
            sessions
                .values()
                .find(|s| s.profile_dir == profile_dir)
                .map(|s| s.tab.clone())
        };
        let Some(tab) = tab else {
            return String::new();
        };
        tokio::task::spawn_blocking(move || super::dump_cookies_as_json(&tab))
            .await
            .unwrap_or_default()
    }

    /// 统计 sessions map 中隶属同一 profile 的 session 数量。保留为 `pub(crate)` helper
    /// 是为了让"引擎是否还有某 profile 的 session"这个语义判断有单一公开入口。
    #[cfg(test)]
    pub(crate) fn sessions_count_for_profile(&self, profile_dir: &str) -> usize {
        let sessions = self.sessions.lock_recover();
        count_sessions_with_profile(&sessions, profile_dir)
    }
}
