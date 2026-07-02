use std::sync::Arc;

use actix_web::test::TestRequest;
use mediagen_browser_gateway::{BrowserGateway, BrowserGatewayConfig};
use mediagen_plugin_runtime::PluginLoader;

use crate::routes::api_key_allowed;
use crate::store::AppStore;
use crate::ServerState;

#[test]
fn empty_api_key_settings_rejects_openai_requests() {
    let state = test_state("empty-keys");
    let request = TestRequest::default().to_http_request();

    assert!(!api_key_allowed(&state, &request));
}

#[test]
fn configured_api_key_allows_matching_bearer_only() {
    let state = test_state("configured-keys");
    state.store.update_settings(crate::store::SettingsUpdate {
        api_keys: Some("key-a=默认分组".to_string()),
        ..crate::store::SettingsUpdate::default()
    });

    let good = TestRequest::default()
        .insert_header(("authorization", "Bearer key-a"))
        .to_http_request();
    let bad = TestRequest::default()
        .insert_header(("authorization", "Bearer bad-key"))
        .to_http_request();

    assert!(api_key_allowed(&state, &good));
    assert!(!api_key_allowed(&state, &bad));
}

fn test_state(name: &str) -> ServerState {
    let root = std::env::temp_dir().join(format!("mediagen-routes-test-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    ServerState {
        plugin_loader: Arc::new(PluginLoader::empty()),
        browser_gateway: Arc::new(BrowserGateway::new(BrowserGatewayConfig {
            profile_root: root.join("profiles"),
            launch_browser: false,
            ..BrowserGatewayConfig::default()
        })),
        store: Arc::new(AppStore::load(root.join("server-state.json"))),
        oauth_logins: Default::default(),
    }
}
