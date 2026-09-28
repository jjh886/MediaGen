use std::sync::OnceLock;
use std::time::{Duration, Instant};

use actix_web::{web, HttpResponse, Responder};
use mediagen_browser_gateway::{
    BrowserGatewayError, BrowserSession, BrowserSessionCreateRequest, BrowserSessionPurpose,
};
use mediagen_core::PluginManifest;
use serde_json::{json, Value};

use crate::account_login;
use crate::browser_session_public::{public_browser_response, public_browser_session};
use crate::browser_session_setup::{
    prepare_browser_session_create_request, release_relogin_profile,
};
use crate::oauth_login;
use crate::routes::public_account;
use crate::ServerState;

#[path = "browser_session_actions.rs"]
mod actions;
use actions::{
    browser_session_history, cdp_browser_session, click_browser_session, clipboard_browser_session,
    download_browser_session, evaluate_browser_session, input_browser_session,
    navigate_browser_session, resize_browser_session, screenshot_browser_session,
    type_browser_session, upload_browser_session,
};

const CHROME_STABLE_VERSIONS_URL: &str =
    "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions.json";
const CHROME_STABLE_FALLBACK_MAJOR: u16 = 154;
const CHROME_VERSION_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
static CHROME_VERSION_CACHE: OnceLock<tokio::sync::Mutex<Option<(Instant, u16)>>> = OnceLock::new();

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/browser-sessions")
            .route("/fingerprint-options", web::get().to(fingerprint_options))
            .route(
                "",
                web::get().to(crate::browser_session_list::list_browser_sessions),
            )
            .route("", web::post().to(create_browser_session))
            .route("/{id}", web::get().to(get_browser_session))
            .route("/{id}/cancel", web::post().to(cancel_browser_session))
            .route("/{id}/complete", web::post().to(complete_browser_session))
            .route("/{id}/navigate", web::post().to(navigate_browser_session))
            .route("/{id}/history", web::post().to(browser_session_history))
            .route("/{id}/resize", web::post().to(resize_browser_session))
            .route(
                "/{id}/screenshot",
                web::post().to(screenshot_browser_session),
            )
            .route("/{id}/evaluate", web::post().to(evaluate_browser_session))
            .route("/{id}/click", web::post().to(click_browser_session))
            .route("/{id}/type", web::post().to(type_browser_session))
            .route("/{id}/upload", web::post().to(upload_browser_session))
            .route("/{id}/download", web::post().to(download_browser_session))
            .route("/{id}/cdp", web::post().to(cdp_browser_session))
            .route("/{id}/input", web::post().to(input_browser_session))
            .route("/{id}/clipboard", web::post().to(clipboard_browser_session))
            .route(
                "/{id}/events",
                web::get().to(crate::browser_session_events::browser_session_events),
            )
            .route(
                "/{id}/stream",
                web::get().to(crate::browser_session_stream::browser_session_stream),
            ),
    );
}

async fn fingerprint_options(state: web::Data<ServerState>) -> impl Responder {
    match state
        .browser_gateway
        .installed_browser_major_version()
        .await
    {
        Ok(installed_version) => {
            let stable_version = latest_stable_chrome_major().await.max(installed_version);
            HttpResponse::Ok().json(json!({
                "data": {
                    "browser_version": stable_version.to_string(),
                    "installed_browser_version": installed_version.to_string(),
                    "os": match std::env::consts::OS {
                        "macos" => "macos",
                        "linux" => "linux",
                        "windows" => "windows",
                        _ => "windows",
                    }
                }
            }))
        }
        Err(error) => gateway_error_response(error),
    }
}

async fn latest_stable_chrome_major() -> u16 {
    let cache = CHROME_VERSION_CACHE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut cached = cache.lock().await;
    if let Some((checked_at, major)) = *cached {
        if checked_at.elapsed() < CHROME_VERSION_CACHE_TTL {
            return major;
        }
    }

    let result: Result<u16, String> = async {
        let response = reqwest::Client::new()
            .get(CHROME_STABLE_VERSIONS_URL)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        let body = response
            .json::<Value>()
            .await
            .map_err(|error| error.to_string())?;
        parse_stable_chrome_major(&body).ok_or_else(|| "Chrome Stable 版本格式无效".to_string())
    }
    .await;
    match result {
        Ok(major) => {
            *cached = Some((Instant::now(), major));
            major.max(CHROME_STABLE_FALLBACK_MAJOR)
        }
        Err(error) => {
            tracing::warn!(%error, "读取 Chrome Stable 版本失败");
            cached
                .map(|(_, major)| major)
                .unwrap_or(CHROME_STABLE_FALLBACK_MAJOR)
                .max(CHROME_STABLE_FALLBACK_MAJOR)
        }
    }
}

fn parse_stable_chrome_major(body: &Value) -> Option<u16> {
    let version = body.pointer("/channels/Stable/version")?.as_str()?;
    let major = version.split('.').next()?.parse::<u16>().ok()?;
    (major >= 100 && version.split('.').count() == 4).then_some(major)
}

pub async fn create_browser_session(
    state: web::Data<ServerState>,
    request: web::Json<BrowserSessionCreateRequest>,
) -> impl Responder {
    let mut request = request.into_inner();
    if let Err(response) = prepare_browser_session_create_request(&state, &mut request) {
        return response;
    }
    if let Some(manifest) = state.plugin_loader.get_plugin(&request.platform) {
        if let Err(message) = oauth_login::prepare_oauth_request(manifest, &mut request) {
            return HttpResponse::BadRequest().json(json!({ "ok": false, "error": message }));
        }
    }
    if let Err(err) = release_relogin_profile(&state, &request).await {
        return gateway_error_response(err);
    }
    if let Some(manifest) = state.plugin_loader.get_plugin(&request.platform) {
        if let Err(message) =
            crate::plugin_login_prepare::prepare(&state, manifest, &mut request).await
        {
            return HttpResponse::BadRequest().json(json!({"ok": false, "error": message}));
        }
    }
    let oauth_state = oauth_login::oauth_meta(&request.metadata).map(|meta| meta.state);
    if let Some(oauth_state) = oauth_state.as_deref() {
        if let Err(message) = state.oauth_logins.start_login(state.clone(), oauth_state) {
            return HttpResponse::Conflict().json(json!({ "ok": false, "error": message }));
        }
    }
    let browser_profile_dir = request
        .browser_profile_dir
        .clone()
        .or_else(|| request.profile_dir.clone());
    let result = if let Some(profile_dir) = browser_profile_dir.as_deref() {
        match state.browser_gateway.profile_open_lock(profile_dir) {
            Ok(lock) => {
                let _guard = lock.lock().await;
                match state
                    .browser_gateway
                    .create_profile_tab_session(request.clone())
                    .await
                {
                    Ok(Some(response)) => Ok(response),
                    Ok(None) => state.browser_gateway.create_session(request).await,
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    } else {
        state.browser_gateway.create_session(request).await
    };
    match result {
        Ok(response) => HttpResponse::Ok().json(public_browser_response(response)),
        Err(err) => {
            if let Some(oauth_state) = oauth_state {
                state.oauth_logins.finish_login(&oauth_state).await;
            }
            gateway_error_response(err)
        }
    }
}

pub async fn get_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    match state.browser_gateway.get_session(&path.into_inner()) {
        Ok(session) => HttpResponse::Ok().json(serde_json::json!({
            "ok": true,
            "session": public_browser_session(session),
        })),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn cancel_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    let session_id = path.into_inner();
    let oauth_state = state
        .browser_gateway
        .get_session(&session_id)
        .ok()
        .and_then(|session| oauth_login::oauth_meta(&session.metadata))
        .map(|meta| meta.state);
    let result = state.browser_gateway.cancel_session(&session_id).await;
    if let Some(oauth_state) = oauth_state {
        state.oauth_logins.finish_login(&oauth_state).await;
    }
    match result {
        Ok(response) => HttpResponse::Ok().json(public_browser_response(response)),
        Err(err) => gateway_error_response(err),
    }
}

pub async fn complete_browser_session(
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> impl Responder {
    let session_id = path.into_inner();
    let current_session = match state.browser_gateway.get_session(&session_id) {
        Ok(session) => session,
        Err(err) => return gateway_error_response(err),
    };
    let verification_context = verification_completion_context(&current_session);
    let (login_profile, manifest) = match prepare_login_completion(&state, &current_session).await {
        Ok(context) => context,
        Err(response) => return response,
    };
    let oauth_state = oauth_login::oauth_meta(&current_session.metadata).map(|meta| meta.state);

    match state.browser_gateway.complete_session(&session_id).await {
        Ok(response) => {
            if let Some(oauth_state) = oauth_state {
                state.oauth_logins.finish_login(&oauth_state).await;
            }
            if let Some((task_id, account_id)) = verification_context {
                finish_human_verification(&state, &task_id, &account_id);
                return HttpResponse::Ok().json(serde_json::json!({
                    "ok": true,
                    "session": public_browser_session(response.session),
                    "account": account_id
                        .and_then(|id| state.store.account_by_id(&id))
                        .map(public_account),
                }));
            }
            let account = match (manifest.as_ref(), login_profile) {
                (Some(manifest), Some(profile)) => state
                    .store
                    .upsert_from_browser_session_with_profile(&response.session, manifest, profile),
                _ => state.store.upsert_from_browser_session(&response.session),
            };
            let public_account = account.map(public_account);
            HttpResponse::Ok().json(serde_json::json!({
                "ok": true,
                "session": public_browser_session(response.session),
                "account": public_account,
            }))
        }
        Err(err) => gateway_error_response(err),
    }
}

/// 登录会话完成前统一校验登录态并采集插件声明的凭据，避免完成会话后再访问已关闭的浏览器。
async fn prepare_login_completion(
    state: &ServerState,
    session: &BrowserSession,
) -> Result<
    (
        Option<account_login::AccountLoginProfile>,
        Option<PluginManifest>,
    ),
    HttpResponse,
> {
    let profile = if account_login::requires_login_verification(state, session) {
        Some(
            account_login::verify_browser_session_login(state, session)
                .await
                .map_err(login_conflict_response)?,
        )
    } else {
        None
    };
    let manifest = profile
        .as_ref()
        .and_then(|_| state.plugin_loader.get_plugin(&session.platform).cloned());
    if let Some(manifest) = manifest.as_ref() {
        crate::account_login_credentials::capture_if_configured(state, session, manifest)
            .await
            .map_err(login_conflict_response)?;
    }
    Ok((profile, manifest))
}

fn login_conflict_response(message: String) -> HttpResponse {
    HttpResponse::Conflict().json(serde_json::json!({
        "ok": false,
        "error": message,
    }))
}

fn verification_completion_context(session: &BrowserSession) -> Option<(String, Option<String>)> {
    if session.purpose != BrowserSessionPurpose::HumanVerification {
        return None;
    }
    let task_id = session.task_id.as_deref()?.trim();
    if task_id.is_empty() {
        return None;
    }
    Some((task_id.to_string(), session.account_id.clone()))
}

fn finish_human_verification(state: &ServerState, task_id: &str, account_id: &Option<String>) {
    if let Some(id) = account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        state.store.mark_account_state(id, "available");
    }
    state.store.resume_task_after_verification(task_id);
    state.store.add_task_event(
        task_id,
        "info",
        "人工验证已完成，任务已恢复为可重试状态".to_string(),
    );
    state.store.add_audit_log(
        "account",
        "info",
        "人工验证完成".to_string(),
        format!("task={task_id}"),
        Some(task_id.to_string()),
    );
}

pub(crate) fn gateway_error_response(err: BrowserGatewayError) -> HttpResponse {
    let status = match err {
        BrowserGatewayError::SessionNotFound(_) => actix_web::http::StatusCode::NOT_FOUND,
        BrowserGatewayError::SessionClosed(_) => actix_web::http::StatusCode::CONFLICT,
        BrowserGatewayError::BrowserRuntimeUnavailable(_) => {
            actix_web::http::StatusCode::PRECONDITION_FAILED
        }
        BrowserGatewayError::BrowserLaunch(_) | BrowserGatewayError::Cdp(_) => {
            actix_web::http::StatusCode::BAD_GATEWAY
        }
        BrowserGatewayError::StatePoisoned => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
    };

    HttpResponse::build(status).json(serde_json::json!({
        "ok": false,
        "error": err.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::parse_stable_chrome_major;
    use serde_json::json;

    #[test]
    fn accepts_only_stable_four_part_chrome_versions() {
        assert_eq!(
            parse_stable_chrome_major(&json!({
                "channels": { "Stable": { "version": "154.0.8037.57" } }
            })),
            Some(154)
        );
        assert_eq!(
            parse_stable_chrome_major(&json!({
                "channels": { "Beta": { "version": "155.0.8059.12" } }
            })),
            None
        );
        assert_eq!(
            parse_stable_chrome_major(&json!({
                "channels": { "Stable": { "version": "bad-version" } }
            })),
            None
        );
    }
}
