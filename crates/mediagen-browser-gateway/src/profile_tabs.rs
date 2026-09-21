use std::sync::Arc;

use tokio::sync::Mutex as AsyncMutex;

use crate::runtime::{
    close_page_target, create_page_target, get_best_page_target, get_page_target,
};
use crate::{
    BrowserGateway, BrowserGatewayError, BrowserSessionCreateRequest, BrowserSessionResponse,
    BrowserSessionRuntime, BrowserSessionStatus,
};

impl BrowserGateway {
    pub(crate) fn start_target_watcher(&self, id: String, runtime: BrowserSessionRuntime) {
        let gateway = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let Ok(session) = gateway.get_session(&id) else {
                    break;
                };
                if session.status != BrowserSessionStatus::Active {
                    break;
                }
                let Ok(current) =
                    get_page_target(runtime.browser.debugging_port, &session.target_id).await
                else {
                    continue;
                };
                if current.is_some() {
                    continue;
                }
                let Ok(target) = get_best_page_target(runtime.browser.debugging_port).await else {
                    continue;
                };
                if target.id != session.target_id {
                    let _ = gateway.attach_target(&id, runtime.clone(), target).await;
                }
            }
        });
    }

    /// 同一资料目录的首次启动必须串行，后续并发任务才能安全复用进程并创建新 Tab。
    pub fn profile_open_lock(
        &self,
        profile_dir: &str,
    ) -> Result<Arc<AsyncMutex<()>>, BrowserGatewayError> {
        let mut locks = self
            .profile_open_locks
            .lock()
            .map_err(|_| BrowserGatewayError::StatePoisoned)?;
        Ok(locks
            .entry(profile_dir.trim().to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone())
    }

    /// 若同资料目录已有 CloakBrowser，则在该进程中新建独立 Tab。
    ///
    /// 返回 `None` 表示这是该资料目录的首个任务，应由调用方启动新浏览器进程。
    pub async fn create_profile_tab_session(
        &self,
        request: BrowserSessionCreateRequest,
    ) -> Result<Option<BrowserSessionResponse>, BrowserGatewayError> {
        let session = self.build_session(request);
        let Some(runtime) = self.reserve_shared_runtime(&session)? else {
            return Ok(None);
        };
        let target = match create_page_target(runtime.browser.debugging_port).await {
            Ok(target) => target,
            Err(error) => {
                self.discard_reserved_runtime(&session.id, &error.to_string())?;
                return Err(error);
            }
        };
        if let Err(error) = self
            .attach_target(&session.id, runtime.clone(), target.clone())
            .await
        {
            self.mark_session_failed(&session.id, &error.to_string())?;
            let _ = self.stop_session_runtime(&session.id, &target.id).await;
            return Err(error);
        }
        self.start_target_watcher(session.id.clone(), runtime);
        Ok(Some(BrowserSessionResponse {
            ok: true,
            session: self.get_session(&session.id)?,
        }))
    }

    fn reserve_shared_runtime(
        &self,
        session: &crate::BrowserSession,
    ) -> Result<Option<BrowserSessionRuntime>, BrowserGatewayError> {
        let mut state = self.lock_state()?;
        if state.sessions.values().any(|existing| {
            existing.status == BrowserSessionStatus::Active
                && existing.profile_dir.trim() == session.profile_dir.trim()
                && crate::proxy::runtime_signature(existing)
                    != crate::proxy::runtime_signature(session)
        }) {
            return Err(BrowserGatewayError::BrowserLaunch(
                "代理或浏览器配置已变更，请结束该账号的旧浏览器会话后重试；不会沿用旧代理".into(),
            ));
        }
        let shared_browser = state.sessions.iter().find_map(|(id, existing)| {
            (existing.status == BrowserSessionStatus::Active
                && existing.profile_dir.trim() == session.profile_dir.trim())
            .then(|| {
                state
                    .runtimes
                    .get(id)
                    .map(|runtime| runtime.browser.clone())
            })
            .flatten()
        });
        let Some(browser) = shared_browser else {
            return Ok(None);
        };
        let runtime = BrowserSessionRuntime {
            browser,
            client: Arc::new(AsyncMutex::new(None)),
        };
        state.sessions.insert(session.id.clone(), session.clone());
        state.runtimes.insert(session.id.clone(), runtime.clone());
        Self::push_event_locked(
            &mut state,
            &session.id,
            "status",
            serde_json::json!(session),
        );
        Ok(Some(runtime))
    }

    fn discard_reserved_runtime(&self, id: &str, message: &str) -> Result<(), BrowserGatewayError> {
        self.mark_session_failed(id, message)?;
        self.remove_runtime(id)?;
        Ok(())
    }

    /// 关闭当前任务的 Tab；只有它是进程最后一个会话时才停止整个浏览器。
    pub(crate) async fn stop_session_runtime(
        &self,
        id: &str,
        target_id: &str,
    ) -> Result<(), BrowserGatewayError> {
        let (runtime, shared) = {
            let mut state = self.lock_state()?;
            let Some(runtime) = state.runtimes.remove(id) else {
                return Ok(());
            };
            let port = runtime.browser.debugging_port;
            let shared = state
                .runtimes
                .values()
                .any(|other| other.browser.debugging_port == port);
            (runtime, shared)
        };
        if shared {
            // Tab 可能已被页面或用户先行关闭；清理失败不应把已完成的生成任务改成失败。
            if let Err(error) = close_page_target(runtime.browser.debugging_port, target_id).await {
                eprintln!("[mediagen-browser] 关闭任务 Tab 失败 target_id={target_id}: {error}");
            }
            Ok(())
        } else {
            runtime.browser.stop().await;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_profile_uses_same_open_lock() {
        let gateway = BrowserGateway::new(Default::default());
        let first = gateway.profile_open_lock("/tmp/profile").unwrap();
        let second = gateway.profile_open_lock("/tmp/profile").unwrap();
        let other = gateway.profile_open_lock("/tmp/other").unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &other));
    }

    #[test]
    fn active_profile_cannot_reuse_a_different_proxy() {
        let gateway = BrowserGateway::new(Default::default());
        let mut previous = gateway.build_session(BrowserSessionCreateRequest::default());
        previous.profile_dir = "/tmp/proxy-profile".into();
        let mut next = previous.clone();
        next.metadata.insert(
            "proxy_server".into(),
            serde_json::json!("http://proxy.test:8080"),
        );
        gateway.insert_session(previous).unwrap();
        assert!(gateway.reserve_shared_runtime(&next).is_err());
    }
}
