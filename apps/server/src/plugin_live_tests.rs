use std::{collections::HashMap, path::PathBuf, sync::Arc};

use mediagen_core::PluginManifest;
use mediagen_plugin_runtime::run_js_step;
use serde_json::{json, Value};

// 显式指定私有测试目录才执行；不启动服务器、不在日志中输出凭据或产物。
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires an explicitly configured live account and consumes generation quota"]
async fn configured_plugin_live_flow() {
    let plugin = PathBuf::from(std::env::var("MEDIAGEN_LIVE_PLUGIN_DIR").unwrap());
    let profile = PathBuf::from(std::env::var("MEDIAGEN_LIVE_PROFILE_DIR").unwrap());
    let flow = std::env::var("MEDIAGEN_LIVE_FLOW").unwrap();
    let manifest: PluginManifest =
        serde_json::from_slice(&std::fs::read(plugin.join("manifest.json")).unwrap()).unwrap();
    let mut context: Value =
        serde_json::from_str(&std::env::var("MEDIAGEN_LIVE_CONTEXT").unwrap()).unwrap();
    context["profile_dir"] = json!(profile);
    context["workspace_dir"] = json!(profile);
    context["scripts_dir"] = json!(plugin.join("scripts"));
    let metadata = HashMap::from([
        ("browser_os".into(), json!("windows")),
        ("browser_version".into(), json!("154")),
    ]);
    let host = crate::server_plugin_host::ServerPluginHost::new_with_metadata(
        std::env::var("MEDIAGEN_LIVE_PROXY").unwrap_or_default(),
        &metadata,
    )
    .with_storage(
        Arc::new(crate::generation_queue::CredentialCipher::new(&profile).unwrap()),
        profile.to_str().unwrap(),
        &manifest,
    )
    .with_http_profile(&manifest)
    .with_events(None, format!("live-{}", uuid::Uuid::new_v4()));
    let mut results = Vec::new();
    for step in &manifest.flows[&flow].steps {
        let prepared = crate::flow_runtime::prepare_flow_step(step, &mut context, &mut results)
            .unwrap()
            .unwrap();
        let mut script = prepared.params["script"].as_str().unwrap().to_string();
        if let Ok(path) = std::env::var("MEDIAGEN_LIVE_PRELUDE") {
            script.push_str(&std::fs::read_to_string(path).unwrap());
        }
        let execution = run_js_step(script, context.clone(), host.clone());
        let timeout = std::env::var("MEDIAGEN_LIVE_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(180);
        let outcome =
            tokio::time::timeout(std::time::Duration::from_secs(timeout), execution).await;
        let saved = host.checkpoint(&json!({})).await.unwrap();
        println!(
            "live checkpoint: {}",
            json!({"phase":saved["phase"],"received_text_length":saved["received_text_length"],"received_frame_length":saved["received_frame_length"]})
        );
        let value = outcome.expect("live probe timed out").unwrap();
        crate::flow_runtime::record_flow_result(
            &mut context,
            &mut results,
            &prepared.params,
            value,
        );
    }
    let result = results.last().unwrap();
    assert!(!crate::flow_runtime::flow_result_is_error(result));
    std::fs::write(
        profile.join("live-result.json"),
        serde_json::to_vec(result).unwrap(),
    )
    .unwrap();
    println!(
        "live flow complete: {flow}; result keys: {:?}",
        result.as_object().map(|v| v.keys().collect::<Vec<_>>())
    );
}
