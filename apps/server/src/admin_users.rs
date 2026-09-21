use crate::{admin_password, admin_users_store::UserUpdate, ServerState};
use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::json;

pub(crate) fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/admin-users", web::get().to(list))
        .route("/api/admin-users", web::post().to(create))
        .route("/api/admin-users/{id}", web::patch().to(update))
        .route("/api/admin-users/{id}", web::delete().to(delete))
        .route("/api/admin-users/{id}/password", web::post().to(password));
}
fn actor(state: &ServerState, request: &HttpRequest) -> Result<Option<String>, HttpResponse> {
    if !state.admin.enabled {
        return Err(
            HttpResponse::NotFound().json(json!({"error":"后台用户管理仅在 Linux 服务端启用"}))
        );
    }
    if let Some(user) = crate::admin_auth::request_user(request, state) {
        return Ok(Some(user.id));
    }
    if crate::admin_access::http_token_matches(request) {
        return Ok(None);
    }
    Err(HttpResponse::Unauthorized().json(json!({"error":"请先登录管理员账号"})))
}
fn failure(message: String) -> HttpResponse {
    let status = if message == "用户不存在" {
        404
    } else if message.contains("存储") || message.contains("保存或读取") {
        500
    } else {
        400
    };
    HttpResponse::build(actix_web::http::StatusCode::from_u16(status).unwrap())
        .json(json!({"error":message}))
}
async fn list(state: web::Data<ServerState>, request: HttpRequest) -> HttpResponse {
    if let Err(response) = actor(&state, &request) {
        return response;
    }
    match state.admin.list() {
        Ok(users) => HttpResponse::Ok().json(json!({"data":users})),
        Err(error) => failure(error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateUser {
    username: String,
    #[serde(default)]
    display_name: String,
    password: String,
}
async fn create(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Json<CreateUser>,
) -> HttpResponse {
    let who = match actor(&state, &request) {
        Ok(who) => who,
        Err(response) => return response,
    };
    let name = payload.username.trim().to_string();
    let display = payload.display_name.trim().to_string();
    if let Err(error) = admin_password::validate_username(&name)
        .and_then(|_| admin_password::validate_password(&payload.password))
    {
        return failure(error);
    }
    if display.chars().count() > 100 {
        return failure("显示名称不能超过 100 个字符".into());
    }
    let Ok(_permit) = state.admin.hashing.try_acquire() else {
        return HttpResponse::TooManyRequests().json(json!({"error":"密码处理繁忙，请稍后重试"}));
    };
    let admin = state.admin.clone();
    let secret = payload.password.clone();
    match web::block(move || {
        admin_password::hash(&secret).and_then(|hash| admin.create(&name, &display, &hash))
    })
    .await
    {
        Ok(Ok(user)) => {
            audit(&state, who, "新增后台用户", &user.id);
            HttpResponse::Created().json(json!({"data":user}))
        }
        Ok(Err(error)) => failure(error),
        Err(_) => failure("管理员存储不可用".into()),
    }
}
async fn update(
    state: web::Data<ServerState>,
    request: HttpRequest,
    id: web::Path<String>,
    payload: web::Json<UserUpdate>,
) -> HttpResponse {
    let who = match actor(&state, &request) {
        Ok(who) => who,
        Err(response) => return response,
    };
    let mut update = payload.into_inner();
    if let Some(name) = &mut update.username {
        *name = name.trim().into();
        if let Err(error) = admin_password::validate_username(name) {
            return failure(error);
        }
    }
    if let Some(display) = &mut update.display_name {
        *display = display.trim().into();
        if display.chars().count() > 100 {
            return failure("显示名称不能超过 100 个字符".into());
        }
    }
    match state.admin.update(&id, update, who.as_deref()) {
        Ok(user) => {
            audit(&state, who, "编辑后台用户", &id);
            HttpResponse::Ok().json(json!({"data":user}))
        }
        Err(error) => failure(error),
    }
}
async fn delete(
    state: web::Data<ServerState>,
    request: HttpRequest,
    id: web::Path<String>,
) -> HttpResponse {
    let who = match actor(&state, &request) {
        Ok(who) => who,
        Err(response) => return response,
    };
    match state.admin.delete(&id, who.as_deref()) {
        Ok(()) => {
            audit(&state, who, "删除后台用户", &id);
            HttpResponse::Ok().json(json!({"ok":true}))
        }
        Err(error) => failure(error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Password {
    password: String,
}
async fn password(
    state: web::Data<ServerState>,
    request: HttpRequest,
    id: web::Path<String>,
    payload: web::Json<Password>,
) -> HttpResponse {
    let who = match actor(&state, &request) {
        Ok(who) => who,
        Err(response) => return response,
    };
    if let Err(error) = admin_password::validate_password(&payload.password) {
        return failure(error);
    }
    let Ok(_permit) = state.admin.hashing.try_acquire() else {
        return HttpResponse::TooManyRequests().json(json!({"error":"密码处理繁忙，请稍后重试"}));
    };
    let admin = state.admin.clone();
    let secret = payload.password.clone();
    let target = id.to_string();
    match web::block(move || {
        admin_password::hash(&secret).and_then(|hash| admin.password(&target, &hash))
    })
    .await
    {
        Ok(Ok(())) => {
            audit(&state, who, "重置后台用户密码", &id);
            HttpResponse::Ok().json(json!({"ok":true}))
        }
        Ok(Err(error)) => failure(error),
        Err(_) => failure("管理员存储不可用".into()),
    }
}
fn audit(state: &ServerState, who: Option<String>, action: &str, target: &str) {
    state.store.add_audit_log(
        "security",
        "info",
        action.into(),
        format!(
            "actor={}, target={target}",
            who.as_deref().unwrap_or("local-admin")
        ),
        None,
    );
}
