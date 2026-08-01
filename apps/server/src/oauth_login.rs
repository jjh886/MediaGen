use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use actix_web::{dev::ServerHandle, web, App, HttpResponse, HttpServer, Responder};
use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
use mediagen_browser_gateway::{BrowserSessionCreateRequest, BrowserSessionPurpose};
use mediagen_core::PluginManifest;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::ServerState;

const OAUTH_LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct OAuthLoginStore {
    callbacks: Arc<Mutex<HashMap<String, OAuthCallback>>>,
    callback_bind: Arc<String>,
    listener: Arc<Mutex<OAuthListener>>,
}

#[derive(Default)]
struct OAuthListener {
    handle: Option<ServerHandle>,
    active_states: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthCallback {
    pub code: String,
    pub error: String,
    pub error_description: String,
}

#[derive(Debug, Deserialize)]
struct OAuthPkceConfig {
    auth_url: String,
    token_url: String,
    client_id: String,
    scope: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    token_script_file: String,
    #[serde(default)]
    auth_params: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionOAuthMeta {
    pub state: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub token_url: String,
    pub client_id: String,
    pub token_script_file: String,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    state: String,
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

impl OAuthLoginStore {
    pub fn new(callback_bind: String) -> Self {
        Self {
            callbacks: Arc::default(),
            callback_bind: Arc::new(callback_bind),
            listener: Arc::default(),
        }
    }

    /// 只在 OAuth 登录窗口存在时监听固定回调端口，避免与其他本机客户端抢占端口。
    pub fn start_login(
        &self,
        state: web::Data<ServerState>,
        oauth_state: &str,
    ) -> Result<(), String> {
        let mut listener = self.listener.lock().unwrap_or_else(|err| err.into_inner());
        if listener.handle.is_none() {
            let server = callback_server(state, &self.callback_bind).map_err(|err| {
                format!(
                    "OAuth 登录回调端口 {} 无法启动，请先关闭占用该端口的程序后重试：{err}",
                    self.callback_bind
                )
            })?;
            let handle = server.handle();
            actix_web::rt::spawn(server);
            listener.handle = Some(handle);
        }
        listener.active_states.insert(oauth_state.to_string());
        drop(listener);

        // 登录窗口异常退出且没有发出取消请求时，也必须自动释放端口。
        let store = self.clone();
        let oauth_state = oauth_state.to_string();
        actix_web::rt::spawn(async move {
            tokio::time::sleep(OAUTH_LOGIN_TIMEOUT).await;
            store.finish_login(&oauth_state).await;
        });
        Ok(())
    }

    /// 保存有效回调；最后一个登录收到回调后即可停止监听，不必等账号保存完成。
    fn record_callback(&self, query: OAuthCallbackQuery) -> (bool, Option<ServerHandle>) {
        let oauth_state = query.state.clone();
        let stop = {
            let mut listener = self.listener.lock().unwrap_or_else(|err| err.into_inner());
            if !listener.active_states.remove(&oauth_state) {
                return (false, None);
            }
            listener
                .active_states
                .is_empty()
                .then(|| listener.handle.take())
                .flatten()
        };
        let callback = OAuthCallback {
            code: query.code.unwrap_or_default(),
            error: query.error.unwrap_or_default(),
            error_description: query.error_description.unwrap_or_default(),
        };
        self.callbacks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(oauth_state, callback);
        (true, stop)
    }

    pub fn take_callback(&self, state: &str) -> Option<OAuthCallback> {
        self.callbacks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(state)
    }

    /// 登录完成、取消或超时都会走这里；仅最后一个登录会话负责释放端口。
    pub async fn finish_login(&self, state: &str) {
        self.callbacks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(state);
        let stop = {
            let mut listener = self.listener.lock().unwrap_or_else(|err| err.into_inner());
            listener.active_states.remove(state);
            listener
                .active_states
                .is_empty()
                .then(|| listener.handle.take())
                .flatten()
        };
        if let Some(handle) = stop {
            handle.stop(true).await;
        }
    }
}

impl Default for OAuthLoginStore {
    fn default() -> Self {
        Self::new("127.0.0.1:1455".to_string())
    }
}

pub fn prepare_oauth_request(
    manifest: &PluginManifest,
    request: &mut BrowserSessionCreateRequest,
) -> Result<(), String> {
    if !matches!(
        request.purpose,
        BrowserSessionPurpose::AddAccount | BrowserSessionPurpose::ReloginAccount
    ) {
        return Ok(());
    }
    let Some(config) = oauth_config(manifest)? else {
        return Ok(());
    };
    let state = random_token(32);
    let code_verifier = random_token(64);
    let code_challenge = BASE64_URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    let redirect_uri = configured_redirect_uri(&config);
    request.start_url = build_auth_url(&config, &redirect_uri, &state, &code_challenge);
    request.metadata.insert(
        "oauth_pkce".to_string(),
        json!(SessionOAuthMeta {
            state,
            code_verifier,
            redirect_uri,
            token_url: config.token_url,
            client_id: config.client_id,
            token_script_file: config.token_script_file,
        }),
    );
    Ok(())
}

/// 构建临时本机回调入口；调用方只会在添加或重新登录账号时运行它。
fn callback_server(
    state: web::Data<ServerState>,
    bind: &str,
) -> std::io::Result<actix_web::dev::Server> {
    Ok(HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/auth/callback", web::get().to(oauth_callback))
    })
    .bind(bind)?
    .run())
}

pub fn oauth_meta(session_metadata: &HashMap<String, Value>) -> Option<SessionOAuthMeta> {
    session_metadata
        .get("oauth_pkce")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

pub async fn oauth_callback(
    state: web::Data<ServerState>,
    query: web::Query<OAuthCallbackQuery>,
) -> impl Responder {
    let (accepted, stop) = state.oauth_logins.record_callback(query.into_inner());
    if !accepted {
        return HttpResponse::BadRequest()
            .content_type("text/html; charset=utf-8")
            .body("登录会话不存在或已结束，请回到 MediaGen 重新开始。");
    }
    if let Some(handle) = stop {
        // 当前请求返回后再优雅停止，避免回调页面还没收到响应就断开。
        actix_web::rt::spawn(handle.stop(true));
    }
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(callback_page())
}

fn oauth_config(manifest: &PluginManifest) -> Result<Option<OAuthPkceConfig>, String> {
    match manifest.extra.get("login_oauth_pkce") {
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|err| format!("OAuth 登录配置解析失败: {err}")),
        None => Ok(None),
    }
}

fn build_auth_url(
    config: &OAuthPkceConfig,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> String {
    let mut params = vec![
        ("response_type".to_string(), "code".to_string()),
        ("client_id".to_string(), config.client_id.clone()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("scope".to_string(), config.scope.clone()),
        ("code_challenge".to_string(), code_challenge.to_string()),
        ("code_challenge_method".to_string(), "S256".to_string()),
        ("state".to_string(), state.to_string()),
    ];
    params.extend(
        config
            .auth_params
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    format!("{}?{}", config.auth_url, query_string(&params))
}

fn callback_base_url() -> String {
    std::env::var("MEDIAGEN_OAUTH_CALLBACK_BASE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            let port = std::env::var("MEDIAGEN_PORT").unwrap_or_else(|_| "18080".to_string());
            format!("http://127.0.0.1:{port}")
        })
        .trim_end_matches('/')
        .to_string()
}

fn configured_redirect_uri(config: &OAuthPkceConfig) -> String {
    let redirect_uri = config.redirect_uri.trim();
    if redirect_uri.is_empty() {
        callback_base_url() + "/api/oauth/callback"
    } else {
        redirect_uri.to_string()
    }
}

fn random_token(bytes: usize) -> String {
    let mut data = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut data);
    BASE64_URL_SAFE_NO_PAD.encode(data)
}

fn query_string(params: &[(String, String)]) -> String {
    params
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            other => format!("%{other:02X}").chars().collect(),
        })
        .collect()
}

fn callback_page() -> &'static str {
    "<!doctype html><meta charset=\"utf-8\"><title>登录完成</title>\
     <style>body{font-family:-apple-system,BlinkMacSystemFont,Segoe UI,sans-serif;padding:48px;color:#111}</style>\
     <h1>登录已完成</h1><p>MediaGen 正在保存账号信息，这个页面会自动关闭。</p>\
     <script>setTimeout(()=>window.close(),1000)</script>"
}

#[cfg(test)]
mod tests {
    use super::{build_auth_url, configured_redirect_uri, OAuthLoginStore, OAuthPkceConfig};
    use mediagen_plugin_runtime::PluginLoader;
    use std::collections::HashMap;
    use std::net::TcpListener;
    use std::time::Duration;

    fn config(redirect_uri: &str) -> OAuthPkceConfig {
        OAuthPkceConfig {
            auth_url: "https://auth.example/authorize".to_string(),
            token_url: "https://auth.example/token".to_string(),
            client_id: "client".to_string(),
            scope: "openid profile".to_string(),
            redirect_uri: redirect_uri.to_string(),
            token_script_file: String::new(),
            auth_params: HashMap::new(),
        }
    }

    #[test]
    fn explicit_loopback_redirect_is_used_in_authorize_url() {
        let config = config("http://localhost:1455/auth/callback");
        let redirect_uri = configured_redirect_uri(&config);
        let url = build_auth_url(&config, &redirect_uri, "state", "challenge");

        assert_eq!(redirect_uri, "http://localhost:1455/auth/callback");
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
    }

    #[actix_web::test]
    async fn callback_port_only_listens_while_oauth_login_is_active() {
        let bind = unused_loopback_bind();
        let root = std::env::temp_dir().join(format!("mediagen-oauth-{}", uuid::Uuid::new_v4()));
        let mut state = crate::test_fixtures::server_state(root, PluginLoader::empty());
        let logins = OAuthLoginStore::new(bind.clone());
        state.oauth_logins = logins.clone();
        let state = actix_web::web::Data::new(state);

        assert!(!port_is_open(&bind).await);
        logins.start_login(state.clone(), "login-1").unwrap();
        wait_for_port(&bind, true).await;

        // 并发添加账号时共用回调端口，结束其中一个不能影响另一个。
        logins.start_login(state, "login-2").unwrap();
        logins.finish_login("login-1").await;
        assert!(port_is_open(&bind).await);

        logins.finish_login("login-2").await;
        wait_for_port(&bind, false).await;
    }

    fn unused_loopback_bind() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("127.0.0.1:{}", listener.local_addr().unwrap().port())
    }

    async fn wait_for_port(bind: &str, expected_open: bool) {
        for _ in 0..50 {
            if port_is_open(bind).await == expected_open {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("端口 {bind} 的监听状态没有变为 {expected_open}");
    }

    async fn port_is_open(bind: &str) -> bool {
        tokio::net::TcpStream::connect(bind).await.is_ok()
    }
}
