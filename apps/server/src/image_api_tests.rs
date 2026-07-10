use actix_web::{test as actix_test, web, App};
use mediagen_plugin_runtime::PluginLoader;
use serde_json::json;

use crate::access_control::hash_account_keys;
use crate::store::SettingsUpdate;
use crate::test_fixtures::{account_record, server_state};
use crate::ServerState;

#[actix_web::test]
async fn image_generation_failover_switches_to_next_account() {
    let state = failover_state();
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let response = actix_test::call_service(&app, image_request().to_request()).await;

    assert!(response.status().is_success());
    assert_primary_cooldown(&state);
    assert_eq!(
        state.store.list_tasks(Some("fake"))[0]
            .account_id
            .as_deref(),
        Some("acc-backup")
    );
}

#[actix_web::test]
async fn account_api_key_can_call_image_generation() {
    let state = account_key_state();
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let response = actix_test::call_service(
        &app,
        image_request_with_key("account-image-key").to_request(),
    )
    .await;

    assert!(response.status().is_success());
    assert_eq!(
        state.store.list_tasks(Some("fake"))[0]
            .account_id
            .as_deref(),
        Some("acc-account-key")
    );
}

#[actix_web::test]
async fn needs_verification_marks_account_state() {
    let state = verification_state();
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(crate::configure_routes),
    )
    .await;
    let response =
        actix_test::call_service(&app, image_request_with_key("verify-key").to_request()).await;

    assert_eq!(response.status(), actix_web::http::StatusCode::CONFLICT);
    let account = state
        .store
        .list_accounts(Some("fake"))
        .into_iter()
        .find(|account| account.id == "acc-verify")
        .unwrap();
    assert_eq!(account.login_state, "verification_required");
    assert_eq!(
        state.store.list_tasks(Some("fake"))[0].status,
        "verification_required"
    );
}

fn failover_state() -> ServerState {
    let plugins_dir = write_failover_plugin("image-failover");
    let state = server_state(
        test_root("image-failover"),
        PluginLoader::load_from(&plugins_dir).unwrap(),
    );
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("image-key".to_string()),
        ..SettingsUpdate::default()
    });
    state
        .store
        .insert_account(account_record("acc-primary", "fake", "Primary", ""));
    state
        .store
        .insert_account(account_record("acc-backup", "fake", "Backup", ""));
    state
}

fn account_key_state() -> ServerState {
    let plugins_dir = write_failover_plugin("image-account-key");
    let state = server_state(
        test_root("image-account-key"),
        PluginLoader::load_from(&plugins_dir).unwrap(),
    );
    let mut account = account_record("acc-account-key", "fake", "Account Key", "");
    account.api_key = hash_account_keys("account-image-key");
    state.store.insert_account(account);
    state
}

fn verification_state() -> ServerState {
    let plugins_dir = write_verification_plugin("image-verification");
    let state = server_state(
        test_root("image-verification"),
        PluginLoader::load_from(&plugins_dir).unwrap(),
    );
    state.store.update_settings(SettingsUpdate {
        api_keys: Some("verify-key".to_string()),
        ..SettingsUpdate::default()
    });
    state
        .store
        .insert_account(account_record("acc-verify", "fake", "Verify Account", ""));
    state
}

fn image_request() -> actix_web::test::TestRequest {
    image_request_with_key("image-key")
}

fn image_request_with_key(key: &str) -> actix_web::test::TestRequest {
    actix_test::TestRequest::post()
        .uri("/v1/images/generations")
        .insert_header(("authorization", format!("Bearer {key}")))
        .set_json(json!({ "model": "fake-image", "prompt": "draw" }))
}

fn assert_primary_cooldown(state: &ServerState) {
    assert_eq!(
        state
            .store
            .list_accounts(Some("fake"))
            .into_iter()
            .find(|account| account.id == "acc-primary")
            .unwrap()
            .login_state,
        "cooldown"
    );
}

fn write_failover_plugin(name: &str) -> std::path::PathBuf {
    let root = test_root(name).join("plugins");
    let plugin_dir = root.join("fake");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(root.join("plugins.index.json"), r#"{"plugins":["fake"]}"#).unwrap();
    std::fs::write(plugin_dir.join("manifest.json"), failover_manifest()).unwrap();
    root
}

fn write_verification_plugin(name: &str) -> std::path::PathBuf {
    let root = test_root(name).join("plugins");
    let plugin_dir = root.join("fake");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(root.join("plugins.index.json"), r#"{"plugins":["fake"]}"#).unwrap();
    std::fs::write(plugin_dir.join("manifest.json"), verification_manifest()).unwrap();
    root
}

fn failover_manifest() -> &'static str {
    r#"{
      "platform_id": "fake",
      "display_name": "Fake",
      "adapter_mode": "api",
      "models": [{ "model_id": "fake-image", "display_name": "Fake Image" }],
      "failure_strategy": [
        {
          "contains_any": ["rate limit"],
          "category": "account_rate_limited",
          "retry_action": "failover",
          "account_state": "cooldown",
          "usage_consumed": false
        }
      ],
      "flows": {
        "generate_image": {
          "steps": [
            {
              "action": "plugin.run_js",
              "script": "function main(ctx) { if (ctx.account_id === 'acc-primary') return { status: 'error', message: 'rate limit' }; return { images: [{ url: 'data:image/png;base64,aGVsbG8=' }] }; }",
              "save_as": "result"
            }
          ]
        }
      }
    }"#
}

fn verification_manifest() -> &'static str {
    r#"{
      "platform_id": "fake",
      "display_name": "Fake",
      "adapter_mode": "api",
      "workspace_url": "https://example.test/workspace",
      "models": [{ "model_id": "fake-image", "display_name": "Fake Image" }],
      "flows": {
        "generate_image": {
          "steps": [
            {
              "action": "plugin.run_js",
              "script": "function main() { return { status: 'needs_verification', message: 'passkey required' }; }",
              "save_as": "result"
            }
          ]
        }
      }
    }"#
}

fn test_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mediagen-image-api-test-{name}"))
}
