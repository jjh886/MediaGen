use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex, Notify};

use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::types::StepResult;

#[path = "callback_server_engine/control.rs"]
mod control;
#[path = "callback_server_engine/http.rs"]
mod http;

use control::{
    callback_markers, callback_ports, normalize_callback_path, request_owned_callback_stop,
    wait_for_port_to_close,
};
use http::{read_http_request, write_json_response, write_response, ResponseSpec};

#[cfg(test)]
#[path = "callback_server_engine_tests.rs"]
mod tests;

const DEFAULT_MARKER: &str = "mediagen-callback-server";

pub struct CallbackServerEngine {
    servers: Arc<Mutex<HashMap<String, ServerEntry>>>,
    reserved_ports: Arc<Vec<u16>>,
}

struct ServerEntry {
    bind_host: String,
    port: u16,
    path: String,
    pending: Arc<Mutex<VecDeque<CallbackRequest>>>,
    responders: Arc<Mutex<HashMap<String, oneshot::Sender<ResponseSpec>>>>,
    notify: Arc<Notify>,
    stop: Option<oneshot::Sender<()>>,
}

struct CallbackRequest {
    request_id: String,
    method: String,
    target: String,
    path: String,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
    body: String,
    responder: oneshot::Sender<ResponseSpec>,
}

struct ServerRuntime {
    path: String,
    status_path: String,
    stop_path: String,
    marker: String,
    pending: Arc<Mutex<VecDeque<CallbackRequest>>>,
    notify: Arc<Notify>,
}

impl CallbackServerEngine {
    pub fn new() -> Self {
        Self::with_reserved_ports(Vec::new())
    }

    pub fn with_reserved_ports(reserved_ports: Vec<u16>) -> Self {
        Self {
            servers: Arc::new(Mutex::new(HashMap::new())),
            reserved_ports: Arc::new(reserved_ports),
        }
    }

    async fn start(&self, params: &Value) -> Result<StepResult, AppError> {
        let input = StartInput::from_params(params);
        reject_reserved_ports(&input.ports, &self.reserved_ports)?;
        let listener = self.bind_first_available(&input).await?;
        let port = listener.local_addr()?.port();
        reject_reserved_ports(&[port], &self.reserved_ports)?;
        let server_id = uuid::Uuid::new_v4().to_string();
        let pending = Arc::new(Mutex::new(VecDeque::new()));
        let responders = Arc::new(Mutex::new(HashMap::new()));
        let notify = Arc::new(Notify::new());
        let (stop_tx, stop_rx) = oneshot::channel();

        tokio::spawn(run_server(
            listener,
            ServerRuntime {
                path: input.path.clone(),
                status_path: input.status_path.clone(),
                stop_path: input.stop_path.clone(),
                marker: input.marker.clone(),
                pending: pending.clone(),
                notify: notify.clone(),
            },
            stop_rx,
        ));

        self.servers.lock().await.insert(
            server_id.clone(),
            ServerEntry {
                bind_host: input.bind_host.clone(),
                port,
                path: input.path.clone(),
                pending,
                responders,
                notify,
                stop: Some(stop_tx),
            },
        );

        Ok(StepResult {
            success: true,
            data: json!({
                "server_id": server_id,
                "bind_host": input.bind_host,
                "redirect_host": input.redirect_host,
                "port": port,
                "path": input.path,
                "origin": format!("http://{}:{}", input.redirect_host, port),
                "redirect_uri": format!("http://{}:{}{}", input.redirect_host, port, input.path),
            }),
            error: None,
        })
    }

    async fn wait_request(&self, params: &Value) -> Result<StepResult, AppError> {
        let server_id = required_str(params, "server_id", "callback_server.wait_request")?;
        let timeout_ms = params["timeout_ms"].as_u64().unwrap_or(300_000);
        let (pending, responders, notify) = {
            let servers = self.servers.lock().await;
            let entry = servers.get(server_id).ok_or_else(|| {
                AppError::PlatformError(format!("callback server 不存在: {server_id}"))
            })?;
            (
                entry.pending.clone(),
                entry.responders.clone(),
                entry.notify.clone(),
            )
        };

        let request = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            next_request(pending, notify),
        )
        .await
        .map_err(|_| {
            AppError::PlatformError(format!(
                "callback_server.wait_request 超时 ({timeout_ms}ms)"
            ))
        })?
        .ok_or_else(|| AppError::PlatformError("callback server 已停止".into()))?;
        let request_id = request.request_id.clone();
        responders
            .lock()
            .await
            .insert(request_id.clone(), request.responder);

        Ok(StepResult {
            success: true,
            data: json!({
                "server_id": server_id,
                "request_id": request_id,
                "method": request.method,
                "target": request.target,
                "path": request.path,
                "query": request.query,
                "headers": request.headers,
                "body": request.body,
            }),
            error: None,
        })
    }

    async fn respond(&self, params: &Value) -> Result<StepResult, AppError> {
        let server_id = required_str(params, "server_id", "callback_server.respond")?;
        let request_id = required_str(params, "request_id", "callback_server.respond")?;
        let responders = {
            let servers = self.servers.lock().await;
            let entry = servers.get(server_id).ok_or_else(|| {
                AppError::PlatformError(format!("callback server 不存在: {server_id}"))
            })?;
            entry.responders.clone()
        };
        let responder = responders.lock().await.remove(request_id).ok_or_else(|| {
            AppError::PlatformError(format!("callback request 不存在或已响应: {request_id}"))
        })?;
        let spec = ResponseSpec {
            status: params["status"].as_u64().unwrap_or(200).min(599) as u16,
            content_type: params["content_type"]
                .as_str()
                .unwrap_or("text/html; charset=utf-8")
                .to_string(),
            body: params["body"].as_str().unwrap_or("").to_string(),
        };
        let _ = responder.send(spec);
        Ok(StepResult {
            success: true,
            data: json!({ "server_id": server_id, "request_id": request_id, "responded": true }),
            error: None,
        })
    }

    async fn stop(&self, params: &Value) -> Result<StepResult, AppError> {
        let server_id = required_str(params, "server_id", "callback_server.stop")?;
        let stopped = self.remove_server(server_id).await;
        Ok(StepResult {
            success: true,
            data: json!({ "server_id": server_id, "stopped": stopped }),
            error: None,
        })
    }

    async fn remove_server(&self, server_id: &str) -> bool {
        let entry = self.servers.lock().await.remove(server_id);
        if let Some(mut entry) = entry {
            if let Some(stop) = entry.stop.take() {
                let _ = stop.send(());
            }
            true
        } else {
            false
        }
    }

    async fn bind_first_available(&self, input: &StartInput) -> Result<TcpListener, AppError> {
        let mut last_error = None;
        for port in &input.ports {
            if *port != 0 {
                if self
                    .remove_servers_on_port(&input.bind_host, *port, &input.path)
                    .await
                {
                    wait_for_port_to_close(&input.bind_host, *port).await;
                }
                request_owned_callback_stop(
                    &input.bind_host,
                    *port,
                    &input.status_path,
                    &input.stop_path,
                    &input.markers,
                )
                .await;
            }
            match TcpListener::bind((input.bind_host.as_str(), *port)).await {
                Ok(listener) => return Ok(listener),
                Err(e) => last_error = Some(e),
            }
        }
        Err(AppError::PlatformError(format!(
            "callback server 端口不可用: {}:{:?} ({})",
            input.bind_host,
            input.ports,
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )))
    }

    async fn remove_servers_on_port(&self, bind_host: &str, port: u16, path: &str) -> bool {
        let ids = {
            let servers = self.servers.lock().await;
            servers
                .iter()
                .filter(|(_, entry)| {
                    entry.bind_host == bind_host && entry.port == port && entry.path == path
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        let removed = !ids.is_empty();
        for id in ids {
            self.remove_server(&id).await;
        }
        removed
    }
}

#[async_trait]
impl AtomExecutor for CallbackServerEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "callback_server"
    }

    async fn execute(&self, action: &str, params: &Value) -> Result<StepResult, AppError> {
        match action {
            "callback_server.start" => self.start(params).await,
            "callback_server.wait_request" => self.wait_request(params).await,
            "callback_server.respond" => self.respond(params).await,
            "callback_server.stop" => self.stop(params).await,
            _ => Err(AppError::PlatformError(format!(
                "未知的 callback_server 操作: {action}"
            ))),
        }
    }
}

struct StartInput {
    bind_host: String,
    redirect_host: String,
    path: String,
    ports: Vec<u16>,
    status_path: String,
    stop_path: String,
    markers: Vec<String>,
    marker: String,
}

impl StartInput {
    fn from_params(params: &Value) -> Self {
        let markers = callback_markers_with_default(params);
        Self {
            bind_host: string_param(params, "bind_host", "127.0.0.1"),
            redirect_host: string_param(params, "redirect_host", "localhost"),
            path: normalize_callback_path(params["path"].as_str().unwrap_or("/callback")),
            ports: callback_ports(params),
            status_path: normalize_callback_path(
                params["status_path"].as_str().unwrap_or("/callback/status"),
            ),
            stop_path: normalize_callback_path(
                params["stop_path"].as_str().unwrap_or("/callback/stop"),
            ),
            marker: markers
                .first()
                .cloned()
                .unwrap_or_else(|| DEFAULT_MARKER.to_string()),
            markers,
        }
    }
}

fn callback_markers_with_default(params: &Value) -> Vec<String> {
    let markers = callback_markers(params);
    if markers.is_empty() {
        vec![DEFAULT_MARKER.to_string()]
    } else {
        markers
    }
}

async fn run_server(
    listener: TcpListener,
    runtime: ServerRuntime,
    mut stop_rx: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break };
                let runtime = runtime.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_stream(stream, runtime).await {
                        tracing::debug!(error = %e, "callback server request failed");
                    }
                });
            }
        }
    }
}

impl Clone for ServerRuntime {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            status_path: self.status_path.clone(),
            stop_path: self.stop_path.clone(),
            marker: self.marker.clone(),
            pending: self.pending.clone(),
            notify: self.notify.clone(),
        }
    }
}

async fn handle_stream(mut stream: TcpStream, runtime: ServerRuntime) -> Result<(), AppError> {
    let request = match read_http_request(&mut stream).await {
        Ok(request) => request,
        Err(AppError::PlatformError(msg)) if msg == "callback request 为空" => return Ok(()),
        Err(e) => return Err(e),
    };
    if request.path == runtime.status_path {
        return write_json_response(&mut stream, 200, json!({ "marker": runtime.marker })).await;
    }
    if request.path == runtime.stop_path {
        write_json_response(
            &mut stream,
            200,
            json!({ "stopped": true, "marker": runtime.marker }),
        )
        .await?;
        return Ok(());
    }
    if request.path != runtime.path {
        return write_response(
            &mut stream,
            ResponseSpec {
                status: 404,
                content_type: "text/plain; charset=utf-8".to_string(),
                body: "Not Found".to_string(),
            },
        )
        .await;
    }

    let (tx, rx) = oneshot::channel();
    let callback = CallbackRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: request.method,
        target: request.target,
        path: request.path,
        query: request.query,
        headers: request.headers,
        body: request.body,
        responder: tx,
    };
    runtime.pending.lock().await.push_back(callback);
    runtime.notify.notify_one();
    let response = tokio::time::timeout(Duration::from_secs(300), rx)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_else(|| ResponseSpec {
            status: 504,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: "Callback response timeout".to_string(),
        });
    write_response(&mut stream, response).await
}

async fn next_request(
    pending: Arc<Mutex<VecDeque<CallbackRequest>>>,
    notify: Arc<Notify>,
) -> Option<CallbackRequest> {
    loop {
        if let Some(request) = pending.lock().await.pop_front() {
            return Some(request);
        }
        notify.notified().await;
    }
}

fn required_str<'a>(params: &'a Value, key: &str, action: &str) -> Result<&'a str, AppError> {
    params[key]
        .as_str()
        .ok_or_else(|| AppError::PlatformError(format!("{action} 缺少 {key}")))
}

fn reject_reserved_ports(ports: &[u16], reserved_ports: &[u16]) -> Result<(), AppError> {
    if let Some(port) = ports
        .iter()
        .copied()
        .filter(|port| *port != 0)
        .find(|port| reserved_ports.contains(port))
    {
        return Err(AppError::PlatformError(format!(
            "callback server 不能使用应用 API 端口: {port}"
        )));
    }
    Ok(())
}

fn string_param(params: &Value, key: &str, default: &str) -> String {
    params[key]
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(default)
        .to_string()
}
