use std::collections::HashMap;

use serde_json::Value;

pub(crate) fn build_session_metadata(
    mut metadata: HashMap<String, Value>,
    proxy_id: Option<&str>,
    default_proxy_server: Option<&str>,
) -> HashMap<String, Value> {
    if should_apply_default_proxy(&metadata, proxy_id) {
        if let Some(proxy_server) = default_proxy_server {
            metadata.insert(
                "proxy_server".to_string(),
                Value::String(proxy_server.to_string()),
            );
        }
    }
    metadata
}

fn should_apply_default_proxy(metadata: &HashMap<String, Value>, proxy_id: Option<&str>) -> bool {
    !has_proxy_id(proxy_id) && !has_proxy_server(metadata) && !is_direct_proxy(metadata)
}

fn has_proxy_id(proxy_id: Option<&str>) -> bool {
    proxy_id
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn has_proxy_server(metadata: &HashMap<String, Value>) -> bool {
    metadata
        .get("proxy_server")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

fn is_direct_proxy(metadata: &HashMap<String, Value>) -> bool {
    metadata.get("proxy_mode").and_then(Value::as_str) == Some("direct")
}
