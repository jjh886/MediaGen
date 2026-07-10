use std::collections::HashMap;

use mediagen_browser_gateway::{BrowserSession, BrowserSessionResponse};
use serde_json::{Map, Value};

use crate::net_client::proxy_url_without_auth;

pub fn public_browser_session(mut session: BrowserSession) -> BrowserSession {
    sanitize_metadata(&mut session.metadata);
    session
}

pub fn public_browser_response(mut response: BrowserSessionResponse) -> BrowserSessionResponse {
    response.session = public_browser_session(response.session);
    response
}

pub fn public_browser_sessions(sessions: Vec<BrowserSession>) -> Vec<BrowserSession> {
    sessions.into_iter().map(public_browser_session).collect()
}

pub fn public_browser_event_data(mut data: Value) -> Value {
    if let Some(metadata) = data.get_mut("metadata").and_then(Value::as_object_mut) {
        sanitize_metadata_object(metadata);
    }
    data
}

fn sanitize_metadata(metadata: &mut HashMap<String, Value>) {
    if let Some(value) = sanitized_proxy_server(metadata) {
        metadata.insert("proxy_server".to_string(), value);
    }
    metadata.remove("proxy_username");
    metadata.remove("proxy_password");
}

fn sanitized_proxy_server(metadata: &HashMap<String, Value>) -> Option<Value> {
    metadata
        .get("proxy_server")
        .and_then(Value::as_str)
        .map(proxy_url_without_auth)
        .filter(|value| !value.is_empty())
        .map(Value::String)
}

fn sanitize_metadata_object(metadata: &mut Map<String, Value>) {
    if let Some(value) = sanitized_proxy_server_object(metadata) {
        metadata.insert("proxy_server".to_string(), value);
    }
    metadata.remove("proxy_username");
    metadata.remove("proxy_password");
}

fn sanitized_proxy_server_object(metadata: &Map<String, Value>) -> Option<Value> {
    metadata
        .get("proxy_server")
        .and_then(Value::as_str)
        .map(proxy_url_without_auth)
        .filter(|value| !value.is_empty())
        .map(Value::String)
}
