use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde_json::{json, Value};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::cdp::CdpClient;
use crate::runtime::{is_useful_page_url, launch_cloakbrowser, BrowserRuntime, BrowserTarget};
use crate::{native_dialogs, rendering};
use crate::{
    BrowserGatewayConfig, BrowserGatewayError, BrowserSession, BrowserSessionCreateRequest,
    BrowserSessionEvent, BrowserSessionResponse, BrowserSessionStatus, BrowserViewport,
    DEFAULT_BROWSER_NAME, DEFAULT_DEVICE_SCALE_FACTOR,
};

#[derive(Clone)]
pub(crate) struct BrowserSessionRuntime {
    pub(crate) browser: BrowserRuntime,
    pub(crate) client: Arc<AsyncMutex<Option<Arc<CdpClient>>>>,
}

#[derive(Default)]
pub(crate) struct BrowserGatewayState {
    pub(crate) sessions: HashMap<String, BrowserSession>,
    pub(crate) runtimes: HashMap<String, BrowserSessionRuntime>,
    events: HashMap<String, Vec<BrowserSessionEvent>>,
    clipboards: HashMap<String, String>,
    next_sequence: u64,
}

const MAX_EVENTS_PER_SESSION: usize = 220;

#[derive(Clone)]
pub struct BrowserGateway {
    config: BrowserGatewayConfig,
    state: Arc<Mutex<BrowserGatewayState>>,
    pub(crate) profile_open_locks: Arc<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
}

impl BrowserGateway {
    pub async fn installed_browser_major_version(&self) -> Result<u16, BrowserGatewayError> {
        crate::runtime::installed_browser_major_version(&self.config).await
    }

    pub fn profile_root(&self) -> &std::path::Path {
        &self.config.profile_root
    }

    pub fn new(config: BrowserGatewayConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(BrowserGatewayState::default())),
            profile_open_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create_session(
        &self,
        request: BrowserSessionCreateRequest,
    ) -> Result<BrowserSessionResponse, BrowserGatewayError> {
        let mut session = self.build_session(request);
        let session_id = session.id.clone();

        self.insert_session(session.clone())?;

        if self.config.launch_browser {
            match launch_cloakbrowser(&self.config, &session).await {
                Ok((browser, target)) => {
                    let runtime = BrowserSessionRuntime {
                        browser,
                        client: Arc::new(AsyncMutex::new(None)),
                    };
                    self.insert_runtime(&session_id, runtime.clone())?;
                    self.attach_target(&session_id, runtime.clone(), target)
                        .await?;
                    self.start_target_watcher(session_id.clone(), runtime);
                    session = self.get_session(&session_id)?;
                }
                Err(err) => {
                    self.mark_session_failed(&session_id, &err.to_string())?;
                    return Err(err);
                }
            }
        }

        Ok(BrowserSessionResponse { ok: true, session })
    }

    pub fn list_sessions(&self) -> Result<Vec<BrowserSession>, BrowserGatewayError> {
        let state = self.lock_state()?;
        let mut sessions: Vec<_> = state.sessions.values().cloned().collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.created_at));
        Ok(sessions)
    }

    pub fn get_session(&self, id: &str) -> Result<BrowserSession, BrowserGatewayError> {
        let state = self.lock_state()?;
        state
            .sessions
            .get(id)
            .cloned()
            .ok_or_else(|| BrowserGatewayError::SessionNotFound(id.to_string()))
    }

    pub async fn cancel_session(
        &self,
        id: &str,
    ) -> Result<BrowserSessionResponse, BrowserGatewayError> {
        let response = self.close_session(id, BrowserSessionStatus::Cancelled, "status")?;
        self.stop_session_runtime(id, &response.session.target_id)
            .await?;
        Ok(response)
    }

    pub async fn complete_session(
        &self,
        id: &str,
    ) -> Result<BrowserSessionResponse, BrowserGatewayError> {
        let response = self.close_session(id, BrowserSessionStatus::Completed, "status")?;
        self.stop_session_runtime(id, &response.session.target_id)
            .await?;
        Ok(response)
    }

    pub fn events_since(
        &self,
        id: &str,
        after: u64,
    ) -> Result<Vec<BrowserSessionEvent>, BrowserGatewayError> {
        let state = self.lock_state()?;
        if !state.sessions.contains_key(id) {
            return Err(BrowserGatewayError::SessionNotFound(id.to_string()));
        }
        Ok(state
            .events
            .get(id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|event| event.sequence > after)
            .collect())
    }

    pub(crate) fn build_session(&self, request: BrowserSessionCreateRequest) -> BrowserSession {
        let now = Utc::now();
        let session_id = Uuid::new_v4().to_string();
        let url = request.resolved_url();
        let platform = request.resolved_platform();
        let runtime_id = request.resolved_runtime_id();
        let viewport = request.resolved_viewport(&self.config);
        let device_scale_factor = request.resolved_device_scale_factor();
        let metadata = crate::session_metadata::build_session_metadata(
            request.metadata,
            request.proxy_id.as_deref(),
            self.config.default_proxy_server.as_deref(),
        );
        let profile_dir = crate::session_paths::resolve_profile_dir(
            &self.config.profile_root,
            &session_id,
            request.profile_dir.as_deref(),
        );
        let browser_profile_dir = request
            .browser_profile_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&profile_dir)
            .to_string();

        BrowserSession {
            id: session_id,
            purpose: request.purpose,
            status: BrowserSessionStatus::Active,
            platform,
            account_id: request.account_id,
            task_id: request.task_id,
            proxy_id: request.proxy_id,
            start_url: url.clone(),
            url,
            title: String::new(),
            target_id: String::new(),
            headless: request.headless.unwrap_or(true),
            browser_name: DEFAULT_BROWSER_NAME.to_string(),
            runtime_id,
            profile_dir,
            browser_profile_dir,
            viewport,
            device_scale_factor,
            message: "Browser session 正在启动 CloakBrowser".to_string(),
            cookie_count: 0,
            verification_reason: request.verification_reason,
            created_at: now,
            updated_at: now,
            cancelled_at: None,
            completed_at: None,
            metadata,
        }
    }

    pub(crate) async fn attach_target(
        &self,
        id: &str,
        runtime: BrowserSessionRuntime,
        target: BrowserTarget,
    ) -> Result<(), BrowserGatewayError> {
        let (client, mut events) = CdpClient::connect(&target.websocket_url).await?;
        *runtime.client.lock().await = Some(Arc::clone(&client));

        self.update_session(id, |session| {
            session.target_id = target.id.clone();
            if is_useful_page_url(&target.url) {
                session.url = target.url.clone();
            }
            session.title = target.title.clone();
            session.message = "CloakBrowser 已连接".to_string();
            session.updated_at = Utc::now();
        })?;

        // 配置阶段也可能收到投屏帧、代理认证与弹窗事件，必须立即开始消费。
        let gateway = self.clone();
        let session_id = id.to_string();
        let event_client = Arc::clone(&client);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                gateway
                    .handle_cdp_event(&session_id, Arc::clone(&event_client), event)
                    .await;
            }
        });

        client.send("Page.enable", json!({})).await?;
        client.send("Runtime.enable", json!({})).await?;
        client.send("Network.enable", json!({})).await?;
        let session = self.get_session(id)?;
        crate::fingerprint::apply_browser_fingerprint(&client, &session).await?;
        crate::proxy::enable_auth_handling(&client).await?;
        native_dialogs::install_native_dialog_guards(&client).await;
        rendering::configure_rendering(&client, &session).await?;
        if is_useful_page_url(&session.start_url) {
            // 服务端网络可能较慢，初始跳转不能阻塞会话创建，否则用户看不到可接管画面。
            client.send_fire_and_forget("Page.navigate", json!({ "url": session.start_url }))?;
        }

        Ok(())
    }

    async fn handle_cdp_event(&self, id: &str, client: Arc<CdpClient>, event: Value) {
        let method = event
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = event.get("params").cloned().unwrap_or_else(|| json!({}));
        match method {
            "Page.screencastFrame" => {
                if let Some(session_id) = params.get("sessionId").and_then(Value::as_u64) {
                    let _ = client.send_fire_and_forget(
                        "Page.screencastFrameAck",
                        json!({ "sessionId": session_id }),
                    );
                }
                let (viewport, device_scale_factor) = self
                    .get_session(id)
                    .map(|session| (session.viewport, session.device_scale_factor))
                    .unwrap_or((BrowserViewport::default(), DEFAULT_DEVICE_SCALE_FACTOR));
                let _ = self.push_event(
                    id,
                    "frame",
                    json!({
                        "image": params.get("data").cloned().unwrap_or_default(),
                        "metadata": params.get("metadata").cloned().unwrap_or_default(),
                        "viewport": viewport,
                        "device_scale_factor": device_scale_factor,
                    }),
                );
            }
            "Page.frameNavigated" => {
                let frame = params.get("frame").cloned().unwrap_or_else(|| json!({}));
                let is_main = frame.get("parentId").is_none();
                let url = frame.get("url").and_then(Value::as_str).unwrap_or_default();
                if is_main && is_useful_page_url(url) {
                    let _ = self.update_session(id, |session| {
                        session.url = url.to_string();
                        session.updated_at = Utc::now();
                    });
                }
            }
            "Page.javascriptDialogOpening" => {
                native_dialogs::dismiss_javascript_dialog(&client, &params).await;
            }
            "Fetch.requestPaused" => {
                native_dialogs::continue_paused_request(&client, &params).await;
            }
            "Fetch.authRequired" => {
                if let Ok(session) = self.get_session(id) {
                    crate::proxy::continue_auth(&client, &session, &params).await;
                }
            }
            _ => {}
        }
    }

    pub(crate) async fn session_client(
        &self,
        id: &str,
    ) -> Result<Arc<CdpClient>, BrowserGatewayError> {
        let runtime = {
            let state = self.lock_state()?;
            let session = state
                .sessions
                .get(id)
                .ok_or_else(|| BrowserGatewayError::SessionNotFound(id.to_string()))?;
            ensure_active(session)?;
            state
                .runtimes
                .get(id)
                .cloned()
                .ok_or_else(|| BrowserGatewayError::Cdp("浏览器会话尚未连接".to_string()))?
        };
        let client = runtime.client.lock().await.clone();
        client.ok_or_else(|| BrowserGatewayError::Cdp("CDP 尚未连接".to_string()))
    }

    pub(crate) fn insert_session(
        &self,
        session: BrowserSession,
    ) -> Result<(), BrowserGatewayError> {
        let mut state = self.lock_state()?;
        let session_id = session.id.clone();
        state.sessions.insert(session_id.clone(), session.clone());
        Self::push_event_locked(&mut state, &session_id, "status", json!(session));
        Ok(())
    }

    pub(crate) fn insert_runtime(
        &self,
        id: &str,
        runtime: BrowserSessionRuntime,
    ) -> Result<(), BrowserGatewayError> {
        self.lock_state()?.runtimes.insert(id.to_string(), runtime);
        Ok(())
    }

    pub(crate) fn remove_runtime(
        &self,
        id: &str,
    ) -> Result<Option<BrowserSessionRuntime>, BrowserGatewayError> {
        Ok(self.lock_state()?.runtimes.remove(id))
    }

    pub(crate) fn update_session(
        &self,
        id: &str,
        update: impl FnOnce(&mut BrowserSession),
    ) -> Result<BrowserSession, BrowserGatewayError> {
        let mut state = self.lock_state()?;
        let session = state
            .sessions
            .get_mut(id)
            .ok_or_else(|| BrowserGatewayError::SessionNotFound(id.to_string()))?;
        update(session);
        let session = session.clone();
        Self::push_event_locked(&mut state, id, "status", json!(session));
        Ok(session)
    }

    pub(crate) fn mark_session_failed(
        &self,
        id: &str,
        message: &str,
    ) -> Result<(), BrowserGatewayError> {
        self.update_session(id, |session| {
            session.status = BrowserSessionStatus::Failed;
            session.message = message.to_string();
            session.updated_at = Utc::now();
            session.completed_at = Some(session.updated_at);
        })?;
        Ok(())
    }

    fn close_session(
        &self,
        id: &str,
        status: BrowserSessionStatus,
        event: &str,
    ) -> Result<BrowserSessionResponse, BrowserGatewayError> {
        let mut state = self.lock_state()?;
        let session = state
            .sessions
            .get_mut(id)
            .ok_or_else(|| BrowserGatewayError::SessionNotFound(id.to_string()))?;
        ensure_active(session)?;
        let now = Utc::now();
        session.status = status;
        session.updated_at = now;
        session.completed_at = Some(now);
        if matches!(session.status, BrowserSessionStatus::Cancelled) {
            session.cancelled_at = Some(now);
        }
        let session = session.clone();
        Self::push_event_locked(&mut state, id, event, json!(session));
        Ok(BrowserSessionResponse { ok: true, session })
    }

    pub(crate) fn read_clipboard(&self, id: &str) -> Result<String, BrowserGatewayError> {
        Ok(self
            .lock_state()?
            .clipboards
            .get(id)
            .cloned()
            .unwrap_or_default())
    }

    pub(crate) fn write_clipboard_state(
        &self,
        id: &str,
        text: String,
    ) -> Result<(), BrowserGatewayError> {
        self.lock_state()?.clipboards.insert(id.to_string(), text);
        Ok(())
    }

    pub(crate) fn push_event(
        &self,
        id: &str,
        event: &str,
        data: Value,
    ) -> Result<(), BrowserGatewayError> {
        let mut state = self.lock_state()?;
        Self::push_event_locked(&mut state, id, event, data);
        Ok(())
    }

    pub(crate) fn lock_state(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BrowserGatewayState>, BrowserGatewayError> {
        self.state
            .lock()
            .map_err(|_| BrowserGatewayError::StatePoisoned)
    }

    pub(crate) fn push_event_locked(
        state: &mut BrowserGatewayState,
        session_id: &str,
        event: &str,
        data: Value,
    ) {
        if event == "frame" {
            Self::drop_stale_frames(state, session_id);
        }
        state.next_sequence += 1;
        let item = BrowserSessionEvent {
            sequence: state.next_sequence,
            event: event.to_string(),
            data,
            created_at: Utc::now(),
        };
        state
            .events
            .entry(session_id.to_string())
            .or_default()
            .push(item);
        if let Some(events) = state.events.get_mut(session_id) {
            if events.len() > MAX_EVENTS_PER_SESSION {
                let keep_from = events.len().saturating_sub(MAX_EVENTS_PER_SESSION);
                events.drain(0..keep_from);
            }
        }
    }

    fn drop_stale_frames(state: &mut BrowserGatewayState, session_id: &str) {
        if let Some(events) = state.events.get_mut(session_id) {
            events.retain(|item| item.event != "frame");
        }
    }
}

fn ensure_active(session: &BrowserSession) -> Result<(), BrowserGatewayError> {
    if session.status.is_active() {
        Ok(())
    } else {
        Err(BrowserGatewayError::SessionClosed(session.id.clone()))
    }
}
