use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use actix_web::{web, HttpResponse, Responder};
use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
use mediagen_browser_gateway::{BrowserSessionCreateRequest, BrowserSessionPurpose};
use mediagen_core::PluginManifest;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::ServerState;

#[derive(Clone, Default)]
pub struct OAuthLoginStore {
    callbacks: Arc<Mutex<HashMap<String, OAuthCallback>>>,
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
    pub fn record_callback(&self, query: OAuthCallbackQuery) {
        let callback = OAuthCallback {
            code: query.code.unwrap_or_default(),
            error: query.error.unwrap_or_default(),
            error_description: query.error_description.unwrap_or_default(),
        };
        self.callbacks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(query.state, callback);
    }

    pub fn take_callback(&self, state: &str) -> Option<OAuthCallback> {
        self.callbacks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(state)
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
    let redirect_uri = callback_base_url() + "/api/oauth/callback";
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

pub fn oauth_meta(session_metadata: &HashMap<String, Value>) -> Option<SessionOAuthMeta> {
    session_metadata
        .get("oauth_pkce")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

pub async fn oauth_callback(
    state: web::Data<ServerState>,
    query: web::Query<OAuthCallbackQuery>,
) -> impl Responder {
    state.oauth_logins.record_callback(query.into_inner());
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
