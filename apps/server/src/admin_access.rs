use actix_web::{dev::ServiceRequest, http::header, HttpResponse};
use serde_json::json;

use crate::admin_auth;

const ADMIN_TOKEN_ENV: &str = "MEDIAGEN_ADMIN_TOKEN";
const ADMIN_TOKEN_HEADER: &str = "x-mediagen-admin-token";

pub(crate) fn request_allowed(request: &ServiceRequest) -> bool {
    origin_allowed(request.request())
        && (!requires_admin_access(request.path())
            || token_matches(request, admin_token().as_deref())
            || admin_auth::request_has_session(request)
            || loopback_request_allowed(request))
}

pub(crate) fn unauthorized_response() -> HttpResponse {
    HttpResponse::Unauthorized().json(json!({
        "error": "请先登录 MediaGen 管理台"
    }))
}

fn requires_admin_access(path: &str) -> bool {
    path.starts_with("/api/")
        && path != "/api/oauth/callback"
        && !path.starts_with("/api/auth/")
        && !path.starts_with("/api/v2/generation-")
}

fn loopback_request_allowed(request: &ServiceRequest) -> bool {
    request
        .app_data::<actix_web::web::Data<crate::ServerState>>()
        .is_some_and(|state| !state.admin.enabled)
        && loopback_request(request)
}

fn loopback_request(request: &ServiceRequest) -> bool {
    request
        .peer_addr()
        .is_some_and(|address| address.ip().is_loopback())
}

fn token_matches(request: &ServiceRequest, expected: Option<&str>) -> bool {
    let Some(expected) = expected.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    header_token(request)
        .or_else(|| bearer_token(request))
        .is_some_and(|token| crate::admin_password::equal(token.as_bytes(), expected.as_bytes()))
}

fn header_token(request: &ServiceRequest) -> Option<&str> {
    request
        .headers()
        .get(ADMIN_TOKEN_HEADER)?
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn bearer_token(request: &ServiceRequest) -> Option<&str> {
    request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn admin_token() -> Option<String> {
    std::env::var(ADMIN_TOKEN_ENV).ok()
}

pub(crate) fn http_token_matches(request: &actix_web::HttpRequest) -> bool {
    let Some(expected) = admin_token().filter(|v| !v.trim().is_empty()) else {
        return false;
    };
    let supplied = request
        .headers()
        .get(ADMIN_TOKEN_HEADER)
        .and_then(|h| h.to_str().ok())
        .or_else(|| {
            request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.strip_prefix("Bearer "))
        });
    supplied.is_some_and(|value| {
        crate::admin_password::equal(value.trim().as_bytes(), expected.trim().as_bytes())
    })
}
pub(crate) fn origin_allowed(request: &actix_web::HttpRequest) -> bool {
    if !request.path().starts_with("/api/") || request.path().starts_with("/api/v2/") {
        return true;
    }
    let Some(origin) = request.headers().get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    if let Ok(configured) = std::env::var("MEDIAGEN_ADMIN_ORIGIN") {
        return origin.trim_end_matches('/') == configured.trim_end_matches('/');
    }
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let authority = if let Some(port) = url.port() {
        format!("{host}:{port}")
    } else {
        host.to_string()
    };
    request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .is_some_and(|host| host.eq_ignore_ascii_case(&authority))
}

#[cfg(test)]
mod tests {
    use super::{requires_admin_access, token_matches};
    use actix_web::test as actix_test;

    #[test]
    fn admin_access_only_wraps_management_api() {
        assert!(requires_admin_access("/api/accounts"));
        assert!(requires_admin_access("/api/browser-sessions"));
        assert!(!requires_admin_access("/health"));
        assert!(!requires_admin_access("/v1/images/generations"));
        assert!(!requires_admin_access("/api/oauth/callback"));
        assert!(!requires_admin_access("/api/auth/status"));
        assert!(!requires_admin_access("/api/v2/generation-queue"));
    }

    #[test]
    fn token_can_use_header_or_bearer_auth() {
        let header = actix_test::TestRequest::default()
            .insert_header(("x-mediagen-admin-token", "secret"))
            .to_srv_request();
        let bearer = actix_test::TestRequest::default()
            .insert_header(("authorization", "Bearer secret"))
            .to_srv_request();
        let wrong = actix_test::TestRequest::default()
            .insert_header(("x-mediagen-admin-token", "bad"))
            .to_srv_request();

        assert!(token_matches(&header, Some("secret")));
        assert!(token_matches(&bearer, Some("secret")));
        assert!(!token_matches(&wrong, Some("secret")));
        assert!(!token_matches(&header, None));
    }
}
