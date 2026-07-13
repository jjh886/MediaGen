use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use actix_web::cookie::{time::Duration, Cookie, SameSite};
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::access_control::hash_secret;

const ADMIN_USERNAME_ENV: &str = "MEDIAGEN_ADMIN_USERNAME";
const ADMIN_PASSWORD_ENV: &str = "MEDIAGEN_ADMIN_PASSWORD";
const ADMIN_PASSWORD_HASH_ENV: &str = "MEDIAGEN_ADMIN_PASSWORD_HASH";
const ADMIN_SESSION_COOKIE: &str = "mediagen_admin_session";

static ADMIN_SESSIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
struct AdminCredentials {
    username: String,
    password_hash: String,
}

#[derive(Deserialize)]
pub struct LoginPayload {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct AuthStatus {
    configured: bool,
    authenticated: bool,
    username: Option<String>,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/auth/status", web::get().to(status))
        .route("/api/auth/login", web::post().to(login))
        .route("/api/auth/logout", web::post().to(logout));
}

pub(crate) fn admin_login_configured() -> bool {
    credentials().is_some()
}

pub(crate) fn request_has_session(request: &actix_web::dev::ServiceRequest) -> bool {
    request
        .cookie(ADMIN_SESSION_COOKIE)
        .as_ref()
        .and_then(session_value)
        .is_some_and(session_exists)
}

async fn status(request: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json(json!({ "data": status_payload(&request) }))
}

async fn login(request: HttpRequest, payload: web::Json<LoginPayload>) -> impl Responder {
    let Some(credentials) = credentials() else {
        return HttpResponse::ServiceUnavailable().json(json!({
            "error": "服务端未配置管理员登录账号，请先设置 MEDIAGEN_ADMIN_PASSWORD_HASH"
        }));
    };
    if !credentials.matches(&payload.username, &payload.password) {
        return HttpResponse::Unauthorized().json(json!({ "error": "账号或密码不正确" }));
    }

    let session = create_session();
    HttpResponse::Ok()
        .cookie(session_cookie(&session, cookie_secure(&request)))
        .json(json!({
            "data": AuthStatus {
                configured: true,
                authenticated: true,
                username: Some(credentials.username),
            }
        }))
}

async fn logout(request: HttpRequest) -> impl Responder {
    if let Some(cookie) = request.cookie(ADMIN_SESSION_COOKIE) {
        remove_session(cookie.value());
    }
    HttpResponse::Ok()
        .cookie(expired_cookie(cookie_secure(&request)))
        .json(json!({ "ok": true }))
}

fn status_payload(request: &HttpRequest) -> AuthStatus {
    let credentials = credentials();
    let configured = credentials.is_some();
    let authenticated = configured
        && request
            .cookie(ADMIN_SESSION_COOKIE)
            .as_ref()
            .and_then(session_value)
            .is_some_and(session_exists);
    AuthStatus {
        configured,
        authenticated,
        username: authenticated.then(|| credentials.unwrap().username),
    }
}

fn credentials() -> Option<AdminCredentials> {
    #[cfg(test)]
    if let Some(credentials) = test_credentials() {
        return Some(credentials);
    }
    credentials_from_env()
}

fn credentials_from_env() -> Option<AdminCredentials> {
    let password_hash = configured_password_hash()?;
    let username = std::env::var(ADMIN_USERNAME_ENV).unwrap_or_else(|_| "admin".to_string());
    Some(AdminCredentials {
        username,
        password_hash,
    })
}

fn configured_password_hash() -> Option<String> {
    std::env::var(ADMIN_PASSWORD_HASH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var(ADMIN_PASSWORD_ENV)
                .ok()
                .map(|value| hash_secret(value.trim()))
        })
}

impl AdminCredentials {
    fn matches(&self, username: &str, password: &str) -> bool {
        self.username == username.trim() && self.password_hash == hash_secret(password)
    }
}

fn sessions() -> &'static Mutex<HashSet<String>> {
    ADMIN_SESSIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn create_session() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let session = URL_SAFE_NO_PAD.encode(bytes);
    sessions().lock().unwrap().insert(session.clone());
    session
}

fn remove_session(session: &str) {
    sessions().lock().unwrap().remove(session);
}

fn session_exists(session: &str) -> bool {
    sessions().lock().unwrap().contains(session)
}

fn session_value<'a>(cookie: &'a Cookie<'_>) -> Option<&'a str> {
    let value = cookie.value().trim();
    (!value.is_empty()).then_some(value)
}

fn session_cookie(session: &str, secure: bool) -> Cookie<'static> {
    Cookie::build(ADMIN_SESSION_COOKIE, session.to_string())
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::hours(12))
        .finish()
}

fn expired_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build(ADMIN_SESSION_COOKIE, "")
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::seconds(0))
        .finish()
}

fn cookie_secure(request: &HttpRequest) -> bool {
    request.connection_info().scheme() == "https"
        || forwarded_proto(request).is_some_and(|proto| proto.eq_ignore_ascii_case("https"))
}

fn forwarded_proto(request: &HttpRequest) -> Option<&str> {
    request
        .headers()
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
}

#[cfg(test)]
fn test_credentials() -> Option<AdminCredentials> {
    TEST_CREDENTIALS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
}

#[cfg(test)]
fn set_test_credentials(username: &str, password: &str) {
    *TEST_CREDENTIALS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(AdminCredentials {
        username: username.to_string(),
        password_hash: hash_secret(password),
    });
}

#[cfg(test)]
fn clear_test_credentials() {
    *TEST_CREDENTIALS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = None;
}

#[cfg(test)]
static TEST_CREDENTIALS: OnceLock<Mutex<Option<AdminCredentials>>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test as actix_test;

    #[test]
    fn plaintext_password_is_compared_as_hash() {
        let credentials = AdminCredentials {
            username: "admin".to_string(),
            password_hash: hash_secret("secret"),
        };

        assert!(credentials.matches("admin", "secret"));
        assert!(!credentials.matches("admin", "bad"));
        assert!(!credentials.matches("other", "secret"));
    }

    #[test]
    fn session_lifecycle_accepts_only_issued_values() {
        let session = create_session();

        assert!(session_exists(&session));
        assert!(!session_exists("missing-session"));
        remove_session(&session);
        assert!(!session_exists(&session));
    }

    #[test]
    fn secure_cookie_follows_forwarded_https() {
        let request = actix_test::TestRequest::default()
            .insert_header(("x-forwarded-proto", "https"))
            .to_http_request();

        assert!(cookie_secure(&request));
    }

    #[test]
    fn status_reports_authenticated_user() {
        set_test_credentials("admin", "secret");
        let session = create_session();
        let request = actix_test::TestRequest::default()
            .cookie(Cookie::new(ADMIN_SESSION_COOKIE, session.clone()))
            .to_http_request();
        let status = status_payload(&request);

        assert!(status.authenticated);
        assert_eq!(status.username.as_deref(), Some("admin"));
        remove_session(&session);
        clear_test_credentials();
    }
}
