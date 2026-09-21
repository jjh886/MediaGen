use crate::{
    admin_password,
    admin_users_store::{AdminStore, AdminUser},
    ServerState,
};
use actix_web::{
    cookie::{time::Duration, Cookie, SameSite},
    web, HttpRequest, HttpResponse,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::Deserialize;
use serde_json::json;
pub(crate) const COOKIE: &str = "mediagen_admin_session";

pub(crate) fn initialize(root: &std::path::Path) -> Result<AdminStore, String> {
    let enabled = cfg!(target_os = "linux")
        || std::env::var("MEDIAGEN_ADMIN_AUTH").is_ok_and(|value| value == "1");
    let mut store = AdminStore::open(&root.join("admin.sqlite"), enabled)?;
    if enabled && !store.configured() {
        let username = std::env::var("MEDIAGEN_ADMIN_USERNAME").unwrap_or_else(|_| "admin".into());
        let configured_hash = std::env::var("MEDIAGEN_ADMIN_PASSWORD_HASH")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let hash = if let Some(hash) = configured_hash {
            let hash = hash.trim();
            if hash.starts_with("$argon2id$") && argon2::PasswordHash::new(hash).is_ok() {
                Some(hash.into())
            } else if hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()) {
                Some(format!("legacy-sha256:{}", hash.to_lowercase()))
            } else {
                return Err("管理员初始密码哈希格式无效".into());
            }
        } else {
            std::env::var("MEDIAGEN_ADMIN_PASSWORD")
                .ok()
                .filter(|v| !v.is_empty())
                .map(|v| admin_password::hash(&v))
                .transpose()?
        };
        if let Some(hash) = hash {
            admin_password::validate_username(&username)?;
            store.create(&username, "初始管理员", &hash)?;
            store.initialized_from_env = true;
        }
    }
    Ok(store)
}
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/auth/status", web::get().to(status))
        .route("/api/auth/login", web::post().to(login))
        .route("/api/auth/logout", web::post().to(logout))
        .configure(crate::admin_users::configure);
}
pub(crate) fn request_user(request: &HttpRequest, state: &ServerState) -> Option<AdminUser> {
    request
        .cookie(COOKIE)
        .and_then(|cookie| state.admin.session(cookie.value()))
}
pub(crate) fn request_has_session(request: &actix_web::dev::ServiceRequest) -> bool {
    request
        .app_data::<web::Data<ServerState>>()
        .is_some_and(|state| {
            request
                .cookie(COOKIE)
                .is_some_and(|cookie| state.admin.session(cookie.value()).is_some())
        })
}
fn status_value(state: &ServerState, user: Option<AdminUser>) -> serde_json::Value {
    json!({"supported":state.admin.enabled,"initialized_from_env":state.admin.initialized_from_env,"configured":state.admin.configured(),"authenticated":user.is_some(),"username":user.as_ref().map(|u|&u.username),"user_id":user.map(|u|u.id)})
}
async fn status(state: web::Data<ServerState>, request: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("cache-control", "no-store"))
        .json(json!({"data":status_value(&state,request_user(&request,&state))}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginPayload {
    username: String,
    password: String,
}
async fn login(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Json<LoginPayload>,
) -> HttpResponse {
    if !state.admin.enabled || !state.admin.configured() {
        return HttpResponse::ServiceUnavailable()
            .json(json!({"error":"服务端尚未初始化管理员，请配置初始管理员环境变量后重启"}));
    }
    let name = payload.username.trim().to_string();
    if name.len() > 64 || payload.password.len() > 1024 {
        return HttpResponse::Unauthorized().json(json!({"error":"账号或密码不正确"}));
    }
    let peer = request
        .peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".into());
    if !allow_attempt(&state.admin, &peer, &name) {
        return HttpResponse::TooManyRequests()
            .insert_header(("retry-after", "900"))
            .json(json!({"error":"登录尝试过于频繁，请 15 分钟后重试"}));
    }
    let Ok(_permit) = state.admin.hashing.try_acquire() else {
        return HttpResponse::TooManyRequests().json(json!({"error":"登录繁忙，请稍后重试"}));
    };
    let admin = state.admin.clone();
    let password = payload.password.clone();
    let outcome = web::block(move || -> Result<(AdminUser, String), String> {
        let credential = admin.credential(&name)?;
        // 未知用户也执行一次慢哈希，不以快速返回暴露用户名是否存在。
        let Some((user, mut hash)) = credential else {
            let _ = admin_password::hash(&password);
            return Err("账号或密码不正确".into());
        };
        if !admin_password::verify(&password, &hash) || !user.enabled {
            return Err("账号或密码不正确".into());
        }
        if hash.starts_with("legacy-sha256:") {
            let upgraded = admin_password::hash(&password)?;
            admin.replace_password(&user.id, &upgraded, Some(&hash))?;
            hash = upgraded;
        }
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let token = URL_SAFE_NO_PAD.encode(bytes);
        admin.issue_session(&user.id, &hash, &token)?;
        Ok((user, token))
    })
    .await;
    match outcome {
        Ok(Ok((user, token))) => {
            if let Some(old) = request.cookie(COOKIE) {
                state.admin.logout(old.value());
            }
            if let Ok(mut attempts) = state.admin.attempts.lock() {
                attempts.remove(&format!(
                    "user:{}",
                    payload.username.trim().to_ascii_lowercase()
                ));
            }
            state.store.add_audit_log(
                "security",
                "info",
                "管理员登录".into(),
                format!("user_id={}", user.id),
                None,
            );
            HttpResponse::Ok()
                .cookie(session_cookie(&token, cookie_secure(&request)))
                .insert_header(("cache-control", "no-store"))
                .json(json!({"data":status_value(&state,Some(user))}))
        }
        _ => {
            state.store.add_audit_log(
                "security",
                "warn",
                "管理员登录失败".into(),
                "凭据无效或登录状态变化".into(),
                None,
            );
            HttpResponse::Unauthorized().json(json!({"error":"账号或密码不正确"}))
        }
    }
}
fn allow_attempt(store: &AdminStore, peer: &str, name: &str) -> bool {
    let now = chrono::Utc::now().timestamp();
    let Ok(mut attempts) = store.attempts.lock() else {
        return false;
    };
    attempts.retain(|_, (_, at)| now - *at < 900);
    if attempts.len() > 10000 {
        return false;
    }
    let keys = [
        (format!("ip:{peer}"), 60),
        (format!("user:{}", name.to_ascii_lowercase()), 10),
    ];
    if keys
        .iter()
        .any(|(key, limit)| attempts.get(key).is_some_and(|(count, _)| count >= limit))
    {
        return false;
    }
    for (key, _) in keys {
        let entry = attempts.entry(key).or_insert((0, now));
        entry.0 += 1;
    }
    true
}
async fn logout(state: web::Data<ServerState>, request: HttpRequest) -> HttpResponse {
    if let Some(cookie) = request.cookie(COOKIE) {
        state.admin.logout(cookie.value());
    }
    HttpResponse::Ok()
        .cookie(
            Cookie::build(COOKIE, "")
                .path("/")
                .http_only(true)
                .secure(cookie_secure(&request))
                .same_site(SameSite::Lax)
                .max_age(Duration::ZERO)
                .finish(),
        )
        .json(json!({"ok":true}))
}
pub(crate) fn session_cookie(value: &str, secure: bool) -> Cookie<'static> {
    Cookie::build(COOKIE, value.to_string())
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::hours(12))
        .finish()
}
fn cookie_secure(request: &HttpRequest) -> bool {
    request.connection_info().scheme() == "https"
        || request
            .headers()
            .get("x-forwarded-proto")
            .is_some_and(|value| value == "https")
}

#[cfg(test)]
mod tests {
    #[test]
    fn login_rate_limit_is_bounded_and_shared_across_ips_for_same_username() {
        let root = std::env::temp_dir().join(format!(
            "admin-throttle-{}/users.sqlite",
            uuid::Uuid::new_v4()
        ));
        let store = super::AdminStore::open(&root, true).unwrap();
        for i in 0..10 {
            assert!(super::allow_attempt(&store, &format!("ip-{i}"), "Admin"));
        }
        assert!(!super::allow_attempt(&store, "new-ip", "admin"));
        store
            .attempts
            .lock()
            .unwrap()
            .values_mut()
            .for_each(|entry| entry.1 -= 901);
        assert!(super::allow_attempt(&store, "new-ip", "admin"));
    }
}
