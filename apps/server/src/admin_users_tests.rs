use crate::{
    admin_password,
    admin_users_store::{AdminStore, UserUpdate},
};
use actix_web::{test as at, web, App};
use serde_json::json;
use std::sync::Arc;
fn store() -> AdminStore {
    AdminStore::open(
        &std::env::temp_dir().join(format!("admin-test-{}/users.sqlite", uuid::Uuid::new_v4())),
        true,
    )
    .unwrap()
}
#[test]
fn credentials_are_salted_and_invalid_passwords_rejected() {
    let hash = admin_password::hash("correct-password").unwrap();
    assert!(hash.starts_with("$argon2id$"));
    assert_ne!(hash, admin_password::hash("correct-password").unwrap());
    assert!(admin_password::verify("correct-password", &hash));
    assert!(!admin_password::verify("incorrect", &hash));
    assert!(admin_password::validate_password("short").is_err());
    assert!(admin_password::validate_username("bad user").is_err());
}
#[test]
fn user_changes_revoke_sessions_and_protect_last_admin() {
    let store = store();
    let hash = admin_password::hash("correct-password").unwrap();
    let first = store.create("admin", "Administrator", &hash).unwrap();
    assert!(store.create("ADMIN", "Duplicate", &hash).is_err());
    assert!(store.delete(&first.id, None).is_err());
    assert!(store
        .update(
            &first.id,
            UserUpdate {
                enabled: Some(false),
                ..Default::default()
            },
            None
        )
        .is_err());
    let second = store.create("second", "Second", &hash).unwrap();
    store
        .issue_session(&second.id, &hash, "session-token")
        .unwrap();
    assert_eq!(store.session("session-token").unwrap().id, second.id);
    assert!(!serde_json::to_string(&store.list().unwrap())
        .unwrap()
        .contains("argon2"));
    let next = admin_password::hash("changed-password").unwrap();
    store.password(&second.id, &next).unwrap();
    assert!(store
        .replace_password(&second.id, &hash, Some(&hash))
        .is_err());
    assert!(store.session("session-token").is_none());
    assert!(store
        .issue_session(&second.id, &hash, "old-password-race")
        .is_err());
    store.issue_session(&second.id, &next, "new-token").unwrap();
    store
        .update(
            &second.id,
            UserUpdate {
                enabled: Some(false),
                ..Default::default()
            },
            Some(&first.id),
        )
        .unwrap();
    assert!(store.session("new-token").is_none());
    store.delete(&second.id, Some(&first.id)).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
}
#[test]
fn sessions_expire_on_server_and_survive_restart_until_expiry() {
    let root = std::env::temp_dir().join(format!(
        "admin-restart-{}/users.sqlite",
        uuid::Uuid::new_v4()
    ));
    let store = AdminStore::open(&root, true).unwrap();
    let hash = admin_password::hash("correct-password").unwrap();
    let user = store.create("admin", "Admin", &hash).unwrap();
    store.issue_session(&user.id, &hash, "token").unwrap();
    drop(store);
    let restored = AdminStore::open(&root, true).unwrap();
    assert!(restored.session("token").is_some());
    restored
        .db
        .lock()
        .unwrap()
        .execute("UPDATE admin_sessions SET expires_at=0", [])
        .unwrap();
    assert!(restored.session("token").is_none());
}
#[actix_web::test]
async fn linux_login_and_user_management_require_valid_session() {
    let root = std::env::temp_dir().join(format!("admin-api-{}", uuid::Uuid::new_v4()));
    let mut state = crate::test_fixtures::server_state(
        root.clone(),
        mediagen_plugin_runtime::PluginLoader::empty(),
    );
    state.admin = Arc::new(AdminStore::open(&root.join("users.sqlite"), true).unwrap());
    state
        .admin
        .create(
            "admin",
            "Admin",
            &admin_password::hash("correct-password").unwrap(),
        )
        .unwrap();
    let app = at::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::admin_auth::configure),
    )
    .await;
    let denied = at::call_service(
        &app,
        at::TestRequest::get()
            .uri("/api/admin-users")
            .peer_addr("127.0.0.1:1000".parse().unwrap())
            .to_request(),
    )
    .await;
    assert_eq!(denied.status(), 401);
    let response = at::call_service(
        &app,
        at::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(json!({"username":"admin","password":"correct-password"}))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let cookie = response.response().cookies().next().unwrap().into_owned();
    assert!(cookie.http_only().unwrap());
    assert!(cookie.max_age().is_some());
    let response=at::call_service(&app,at::TestRequest::post().uri("/api/admin-users").cookie(cookie.clone()).set_json(json!({"username":"operator","display_name":"操作员","password":"another-password"})).to_request()).await;
    assert_eq!(response.status(), 201);
    let user: serde_json::Value = at::read_body_json(response).await;
    let id = user["data"]["id"].as_str().unwrap();
    assert!(user["data"].get("password_hash").is_none());
    let response = at::call_service(
        &app,
        at::TestRequest::patch()
            .uri(&format!("/api/admin-users/{id}"))
            .cookie(cookie.clone())
            .set_json(json!({"display_name":"新名称"}))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let response = at::call_service(
        &app,
        at::TestRequest::post()
            .uri(&format!("/api/admin-users/{id}/password"))
            .cookie(cookie.clone())
            .set_json(json!({"password":"new-long-password"}))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let response = at::call_service(
        &app,
        at::TestRequest::delete()
            .uri(&format!("/api/admin-users/{id}"))
            .cookie(cookie.clone())
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let response = at::call_service(
        &app,
        at::TestRequest::post()
            .uri("/api/auth/logout")
            .cookie(cookie.clone())
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let response = at::call_service(
        &app,
        at::TestRequest::get()
            .uri("/api/admin-users")
            .cookie(cookie)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), 401);
}
#[test]
fn browser_origins_must_match_management_host() {
    for (origin, expected) in [
        ("http://localhost:1420", true),
        ("https://evil.example", false),
        ("null", false),
    ] {
        let request = at::TestRequest::post()
            .uri("/api/admin-users")
            .insert_header(("host", "localhost:1420"))
            .insert_header(("origin", origin))
            .to_http_request();
        assert_eq!(crate::admin_access::origin_allowed(&request), expected);
    }
}

#[actix_web::test]
async fn desktop_does_not_expose_admin_users_even_from_loopback() {
    let state = crate::test_fixtures::server_state(
        std::env::temp_dir().join(format!("desktop-admin-{}", uuid::Uuid::new_v4())),
        mediagen_plugin_runtime::PluginLoader::empty(),
    );
    let app = at::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(crate::admin_auth::configure),
    )
    .await;
    for method in [actix_web::http::Method::GET, actix_web::http::Method::POST] {
        let response = at::call_service(
            &app,
            at::TestRequest::default()
                .method(method)
                .uri("/api/admin-users")
                .peer_addr("127.0.0.1:1234".parse().unwrap())
                .set_json(json!({"username":"operator","password":"safe-test-password"}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), 404);
    }
}
