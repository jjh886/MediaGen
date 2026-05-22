use std::time::Duration;

use serde_json::Value;
use tokio::net::TcpStream;

use super::DEFAULT_MARKER;

pub(super) fn callback_ports(params: &Value) -> Vec<u16> {
    if let Some(arr) = params["ports"].as_array() {
        let ports = arr
            .iter()
            .filter_map(|v| v.as_u64())
            .filter_map(|v| u16::try_from(v).ok())
            .collect::<Vec<_>>();
        if !ports.is_empty() {
            return ports;
        }
    }
    params["port"]
        .as_u64()
        .and_then(|v| u16::try_from(v).ok())
        .map(|v| vec![v])
        .unwrap_or_else(|| vec![0])
}

pub(super) fn callback_markers(params: &Value) -> Vec<String> {
    if let Some(arr) = params["markers"].as_array() {
        let markers = arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();
        if !markers.is_empty() {
            return markers;
        }
    }
    params["marker"]
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| vec![v.to_string()])
        .unwrap_or_else(|| vec![DEFAULT_MARKER.to_string()])
}

pub(super) fn normalize_callback_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

pub(super) async fn request_owned_callback_stop(
    bind_host: &str,
    port: u16,
    status_path: &str,
    stop_path: &str,
    markers: &[String],
) {
    if !remote_callback_matches_marker(bind_host, port, status_path, markers).await {
        return;
    }
    let url = format!("http://{bind_host}:{port}{stop_path}");
    if let Err(e) = local_http_client().get(url).send().await {
        tracing::debug!(port, error = %e, "请求停止旧 callback server 失败");
    }
    wait_for_port_to_close(bind_host, port).await;
}

async fn remote_callback_matches_marker(
    bind_host: &str,
    port: u16,
    status_path: &str,
    markers: &[String],
) -> bool {
    let url = format!("http://{bind_host}:{port}{status_path}");
    let Ok(resp) = local_http_client().get(url).send().await else {
        return false;
    };
    let Ok(body) = resp.text().await else {
        return false;
    };
    let Ok(data) = serde_json::from_str::<Value>(&body) else {
        return false;
    };
    data["marker"]
        .as_str()
        .is_some_and(|marker| markers.iter().any(|allowed| allowed == marker))
}

pub(super) async fn wait_for_port_to_close(bind_host: &str, port: u16) {
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if TcpStream::connect((bind_host, port)).await.is_err() {
            return;
        }
    }
}

pub(super) fn local_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(500))
        .build()
        .expect("failed to build local callback server client")
}
