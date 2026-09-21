use super::{browser_launch_url, local_bundle_candidate_paths, parse_runtime_port};
use mediagen_core::{bundled_cloakbrowser_candidates, first_existing_browser_binary};

#[test]
fn local_bundle_resolution_reuses_packaged_runtime_and_prefers_install_directory() {
    let root = std::env::temp_dir().join(format!("mediagen-runtime-{}", uuid::Uuid::new_v4()));
    let installed = bundled_cloakbrowser_candidates(&root).remove(0);
    let packaged =
        bundled_cloakbrowser_candidates(&root.join("dist-local/MediaGen-local")).remove(0);
    let resolve = || first_existing_browser_binary(local_bundle_candidate_paths(&root));
    assert_eq!(resolve(), None);

    std::fs::create_dir_all(packaged.parent().unwrap()).unwrap();
    // 目录不能被误认为浏览器可执行文件。
    std::fs::create_dir(&packaged).unwrap();
    let directory_result = resolve();
    std::fs::remove_dir(&packaged).unwrap();
    std::fs::write(&packaged, b"").unwrap();
    let packaged_result = resolve();
    std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
    std::fs::write(&installed, b"").unwrap();
    let installed_result = resolve();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(directory_result, None);
    assert_eq!(packaged_result, Some(packaged));
    assert_eq!(installed_result, Some(installed));
}

#[test]
fn browser_starts_blank_before_cdp_guards_are_ready() {
    assert_eq!(browser_launch_url(), "about:blank");
}

#[test]
fn runtime_port_parser_rejects_invalid_registry_values() {
    assert_eq!(parse_runtime_port("52500\n"), Some(52500));
    assert_eq!(parse_runtime_port("0"), None);
    assert_eq!(parse_runtime_port("not-a-port"), None);
}

#[tokio::test]
async fn browser_reuse_requires_same_proxy_credentials_and_fingerprint() {
    use crate::{BrowserGateway, BrowserGatewayConfig, BrowserSessionCreateRequest};
    let gateway = BrowserGateway::new(BrowserGatewayConfig::default());
    let mut session = gateway.build_session(BrowserSessionCreateRequest::default());
    let root = std::env::temp_dir().join(format!("browser-registry-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    session.profile_dir = root.to_string_lossy().into();
    let path = super::runtime_port_file(&session.profile_dir);
    std::fs::write(&path, "52500").unwrap();
    assert!(!crate::proxy::runtime_config_matches(&path, &session).await);
    super::write_runtime_port(&session, 52500).await.unwrap();
    assert!(crate::proxy::runtime_config_matches(&path, &session).await);
    assert_eq!(
        parse_runtime_port(&std::fs::read_to_string(&path).unwrap()),
        Some(52500)
    );
    session.metadata.insert(
        "proxy_server".into(),
        serde_json::json!("http://user:secret@proxy.test:8080"),
    );
    assert!(!crate::proxy::runtime_config_matches(&path, &session).await);
    super::write_runtime_port(&session, 52500).await.unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("secret"));
    session
        .metadata
        .insert("proxy_password".into(), serde_json::json!("changed"));
    assert!(!crate::proxy::runtime_config_matches(&path, &session).await);
    super::write_runtime_port(&session, 52500).await.unwrap();
    session
        .metadata
        .insert("browser_version".into(), serde_json::json!("131"));
    assert!(!crate::proxy::runtime_config_matches(&path, &session).await);
    std::fs::remove_dir_all(root).unwrap();
}
