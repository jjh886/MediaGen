use crate::{BrowserGateway, BrowserGatewayError, BrowserSessionStatus};

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
                    && session.profile_dir.trim() == profile_dir
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
}
