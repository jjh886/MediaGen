use chrono::Utc;

use crate::{
    BrowserGateway, BrowserGatewayError, BrowserSessionPurpose, BrowserSessionResponse,
    BrowserSessionStatus,
};

impl BrowserGateway {
    /// 资料目录只能由一个 CloakBrowser 独占；重新登录前释放同目录的旧会话。
    pub async fn cancel_active_profile_sessions(
        &self,
        profile_dir: &str,
    ) -> Result<usize, BrowserGatewayError> {
        let profile_dir = profile_dir.trim();
        if profile_dir.is_empty() {
            return Ok(0);
        }
        let session_ids = self
            .list_sessions()?
            .into_iter()
            .filter(|session| {
                session.status == BrowserSessionStatus::Active
                    && session.browser_profile_dir.trim() == profile_dir
            })
            .map(|session| session.id)
            .collect::<Vec<_>>();
        let mut cancelled = 0;
        for session_id in session_ids {
            self.cancel_session(&session_id).await?;
            cancelled += 1;
        }
        Ok(cancelled)
    }

    /// 原子地占用同资料目录下的空闲任务会话，避免并发请求同时复用同一个浏览器。
    ///
    /// 这里只负责通用的“资料目录会话租用”，是否启用常驻模式由上层插件决定。
    pub fn claim_idle_profile_session(
        &self,
        profile_dir: &str,
        account_id: &str,
        task_id: Option<&str>,
        start_url: &str,
    ) -> Result<Option<BrowserSessionResponse>, BrowserGatewayError> {
        let profile_dir = profile_dir.trim();
        if profile_dir.is_empty() {
            return Ok(None);
        }
        let mut state = self.lock_state()?;
        let session_id = state
            .sessions
            .iter()
            .find(|(_, session)| {
                session.status == BrowserSessionStatus::Active
                    && session.purpose == BrowserSessionPurpose::TaskTakeover
                    && session.task_id.is_none()
                    && session.profile_dir.trim() == profile_dir
            })
            .map(|(id, _)| id.clone());
        let Some(session_id) = session_id else {
            return Ok(None);
        };
        let session = state
            .sessions
            .get_mut(&session_id)
            .expect("刚找到的浏览器会话必须仍然存在");
        session.account_id = Some(account_id.to_string());
        session.task_id = task_id.map(str::to_string);
        session.start_url = start_url.to_string();
        session.message = "CloakBrowser 已被新任务复用".to_string();
        session.updated_at = Utc::now();
        let session = session.clone();
        Self::push_event_locked(
            &mut state,
            &session_id,
            "status",
            serde_json::json!(session),
        );
        Ok(Some(BrowserSessionResponse { ok: true, session }))
    }

    /// 释放常驻任务会话：有其他并发任务时只关自己的 Tab，最后一个任务保留为空闲 Tab。
    pub async fn release_session(
        &self,
        id: &str,
    ) -> Result<BrowserSessionResponse, BrowserGatewayError> {
        let (response, should_close_tab) = {
            let mut state = self.lock_state()?;
            let current = state
                .sessions
                .get(id)
                .ok_or_else(|| BrowserGatewayError::SessionNotFound(id.to_string()))?;
            if current.status != BrowserSessionStatus::Active {
                return Err(BrowserGatewayError::SessionClosed(id.to_string()));
            }
            let profile_dir = current.browser_profile_dir.clone();
            let should_close_tab = state.sessions.iter().any(|(session_id, session)| {
                session_id != id
                    && session.status == BrowserSessionStatus::Active
                    && session.task_id.is_some()
                    && session.browser_profile_dir == profile_dir
            });
            let session = state
                .sessions
                .get_mut(id)
                .expect("刚读取的浏览器会话必须仍然存在");
            session.task_id = None;
            session.updated_at = Utc::now();
            if should_close_tab {
                session.status = BrowserSessionStatus::Completed;
                session.completed_at = Some(session.updated_at);
                session.message = "当前任务 Tab 已关闭，其他并发任务继续运行".to_string();
            } else {
                session.message = "CloakBrowser 已空闲，等待下一项任务".to_string();
            }
            let session = session.clone();
            Self::push_event_locked(&mut state, id, "status", serde_json::json!(session));
            (
                BrowserSessionResponse { ok: true, session },
                should_close_tab,
            )
        };
        if should_close_tab {
            self.stop_session_runtime(id, &response.session.target_id)
                .await?;
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use crate::{BrowserGatewayConfig, BrowserSessionCreateRequest};

    use super::*;

    fn test_gateway() -> BrowserGateway {
        BrowserGateway::new(BrowserGatewayConfig {
            launch_browser: false,
            ..BrowserGatewayConfig::default()
        })
    }

    #[tokio::test]
    async fn released_profile_session_can_be_claimed_by_next_task() {
        let gateway = test_gateway();
        let created = gateway
            .create_session(BrowserSessionCreateRequest {
                purpose: BrowserSessionPurpose::TaskTakeover,
                account_id: Some("account-1".to_string()),
                task_id: Some("task-1".to_string()),
                profile_dir: Some("/tmp/profile-1".to_string()),
                ..BrowserSessionCreateRequest::default()
            })
            .await
            .unwrap()
            .session;

        gateway.release_session(&created.id).await.unwrap();
        let claimed = gateway
            .claim_idle_profile_session(
                "/tmp/profile-1",
                "account-1",
                Some("task-2"),
                "https://example.com/workspace",
            )
            .unwrap()
            .expect("空闲会话应能被下一个任务复用")
            .session;

        assert_eq!(claimed.id, created.id);
        assert_eq!(claimed.task_id.as_deref(), Some("task-2"));
        assert_eq!(claimed.start_url, "https://example.com/workspace");
    }

    #[tokio::test]
    async fn busy_profile_session_cannot_be_claimed_twice() {
        let gateway = test_gateway();
        gateway
            .create_session(BrowserSessionCreateRequest {
                purpose: BrowserSessionPurpose::TaskTakeover,
                task_id: Some("task-running".to_string()),
                profile_dir: Some("/tmp/profile-busy".to_string()),
                ..BrowserSessionCreateRequest::default()
            })
            .await
            .unwrap();

        let claimed = gateway
            .claim_idle_profile_session(
                "/tmp/profile-busy",
                "account-1",
                Some("task-2"),
                "https://example.com",
            )
            .unwrap();

        assert!(claimed.is_none());
    }

    #[tokio::test]
    async fn releasing_one_concurrent_tab_keeps_the_other_task_active() {
        let gateway = test_gateway();
        let first = gateway
            .create_session(BrowserSessionCreateRequest {
                purpose: BrowserSessionPurpose::TaskTakeover,
                task_id: Some("task-1".to_string()),
                profile_dir: Some("/tmp/shared-profile".to_string()),
                ..BrowserSessionCreateRequest::default()
            })
            .await
            .unwrap()
            .session;
        let second = gateway
            .create_session(BrowserSessionCreateRequest {
                purpose: BrowserSessionPurpose::TaskTakeover,
                task_id: Some("task-2".to_string()),
                profile_dir: Some("/tmp/shared-profile".to_string()),
                ..BrowserSessionCreateRequest::default()
            })
            .await
            .unwrap()
            .session;

        let released = gateway.release_session(&first.id).await.unwrap().session;

        assert_eq!(released.status, BrowserSessionStatus::Completed);
        assert_eq!(
            gateway.get_session(&second.id).unwrap().status,
            BrowserSessionStatus::Active
        );
        assert_eq!(
            gateway.get_session(&second.id).unwrap().task_id.as_deref(),
            Some("task-2")
        );
    }
}
