use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex, Notify};

#[derive(Clone, Default)]
pub struct CallbackRegistry {
    servers: Arc<Mutex<HashMap<String, ServerEntry>>>,
}

struct ServerEntry {
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

struct ResponseSpec {
    status: u16,
    content_type: String,
    body: String,
}

impl CallbackRegistry {
    pub async fn execute(&self, action: &str, params: &Value) -> Result<Value, String> {
        match action {
            "callback_server.start" => self.start(params).await,
            "callback_server.wait_request" => self.wait_request(params).await,
            "callback_server.respond" => self.respond(params).await,
            "callback_server.stop" => self.stop(params).await,
            _ => Err(format!("未知 callback server 操作: {action}")),
        }
    }

    async fn start(&self, params: &Value) -> Result<Value, String> {
        let bind_host = str_param(params, "bind_host").unwrap_or_else(|| "127.0.0.1".to_string());
        let redirect_host = str_param(params, "redirect_host").unwrap_or_else(|| bind_host.clone());
        let path = normalize_path(
            str_param(params, "path")
                .unwrap_or_else(|| "/callback".to_string())
                .as_str(),
        );
        let port = params["port"].as_u64().unwrap_or(0).min(u16::MAX as u64) as u16;
        let listener = TcpListener::bind((bind_host.as_str(), port))
            .await
            .map_err(|err| format!("callback server 端口不可用: {bind_host}:{port} ({err})"))?;
        let port = listener.local_addr().map_err(|err| err.to_string())?.port();
        let server_id = uuid::Uuid::new_v4().to_string();
        let pending = Arc::new(Mutex::new(VecDeque::new()));
        let responders = Arc::new(Mutex::new(HashMap::new()));
        let notify = Arc::new(Notify::new());
        let (stop_tx, stop_rx) = oneshot::channel();

        tokio::spawn(run_server(
            listener,
            path.clone(),
            pending.clone(),
            notify.clone(),
            stop_rx,
        ));

        self.servers.lock().await.insert(
            server_id.clone(),
            ServerEntry {
                pending,
                responders,
                notify,
                stop: Some(stop_tx),
            },
        );

        Ok(json!({
            "server_id": server_id,
            "bind_host": bind_host,
            "redirect_host": redirect_host,
            "port": port,
            "path": path,
            "origin": format!("http://{redirect_host}:{port}"),
            "redirect_uri": format!("http://{redirect_host}:{port}{path}"),
        }))
    }

    async fn wait_request(&self, params: &Value) -> Result<Value, String> {
        let server_id = required_str(params, "server_id")?;
        let timeout_ms = params["timeout_ms"].as_u64().unwrap_or(300_000);
        let (pending, responders, notify) = {
            let servers = self.servers.lock().await;
            let entry = servers
                .get(&server_id)
                .ok_or_else(|| format!("callback server 不存在: {server_id}"))?;
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
        .map_err(|_| format!("callback_server.wait_request 超时 ({timeout_ms}ms)"))?;
        let request = request.ok_or_else(|| "callback server 已停止".to_string())?;
        let request_id = request.request_id.clone();
        responders
            .lock()
            .await
            .insert(request_id.clone(), request.responder);

        Ok(json!({
            "server_id": server_id,
            "request_id": request_id,
            "method": request.method,
            "target": request.target,
            "path": request.path,
            "query": request.query,
            "headers": request.headers,
            "body": request.body,
        }))
    }

    async fn respond(&self, params: &Value) -> Result<Value, String> {
        let server_id = required_str(params, "server_id")?;
        let request_id = required_str(params, "request_id")?;
        let responders = {
            let servers = self.servers.lock().await;
            servers
                .get(&server_id)
                .ok_or_else(|| format!("callback server 不存在: {server_id}"))?
                .responders
                .clone()
        };
        let responder = responders
            .lock()
            .await
            .remove(&request_id)
            .ok_or_else(|| format!("callback request 不存在或已响应: {request_id}"))?;
        let spec = ResponseSpec {
            status: params["status"].as_u64().unwrap_or(200).min(599) as u16,
            content_type: params["content_type"]
                .as_str()
                .unwrap_or("text/html; charset=utf-8")
                .to_string(),
            body: params["body"].as_str().unwrap_or("").to_string(),
        };
        let _ = responder.send(spec);
        Ok(json!({ "server_id": server_id, "request_id": request_id, "responded": true }))
    }

    async fn stop(&self, params: &Value) -> Result<Value, String> {
        let server_id = required_str(params, "server_id")?;
        let entry = self.servers.lock().await.remove(&server_id);
        if let Some(mut entry) = entry {
            if let Some(stop) = entry.stop.take() {
                let _ = stop.send(());
            }
            return Ok(json!({ "server_id": server_id, "stopped": true }));
        }
        Ok(json!({ "server_id": server_id, "stopped": false }))
    }
}

async fn run_server(
    listener: TcpListener,
    path: String,
    pending: Arc<Mutex<VecDeque<CallbackRequest>>>,
    notify: Arc<Notify>,
    mut stop_rx: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                let pending = pending.clone();
                let notify = notify.clone();
                let path = path.clone();
                tokio::spawn(async move {
                    let _ = handle_stream(stream, &path, pending, notify).await;
                });
            }
        }
    }
}

async fn handle_stream(
    mut stream: TcpStream,
    path: &str,
    pending: Arc<Mutex<VecDeque<CallbackRequest>>>,
    notify: Arc<Notify>,
) -> Result<(), String> {
    let request = read_http_request(&mut stream).await?;
    if request.path != path {
        write_response(
            &mut stream,
            ResponseSpec {
                status: 404,
                content_type: "text/plain; charset=utf-8".to_string(),
                body: "not found".to_string(),
            },
        )
        .await?;
        return Ok(());
    }
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    pending.lock().await.push_back(CallbackRequest {
        request_id,
        method: request.method,
        target: request.target,
        path: request.path,
        query: request.query,
        headers: request.headers,
        body: request.body,
        responder: tx,
    });
    notify.notify_waiters();
    let spec = rx.await.unwrap_or(ResponseSpec {
        status: 200,
        content_type: "text/html; charset=utf-8".to_string(),
        body: String::new(),
    });
    write_response(&mut stream, spec).await
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

struct HttpRequest {
    method: String,
    target: String,
    path: String,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
    body: String,
}

async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut buf = vec![0u8; 64 * 1024];
    let n = stream.read(&mut buf).await.map_err(|err| err.to_string())?;
    if n == 0 {
        return Err("callback request 为空".to_string());
    }
    parse_http_request(&String::from_utf8_lossy(&buf[..n]))
}

fn parse_http_request(raw: &str) -> Result<HttpRequest, String> {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let mut lines = head.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| "callback request 缺少请求行".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let (path, query_raw) = target
        .split_once('?')
        .map(|(path, query)| (path.to_string(), query.to_string()))
        .unwrap_or_else(|| (target.clone(), String::new()));
    let headers = lines.filter_map(split_header).collect::<HashMap<_, _>>();
    Ok(HttpRequest {
        method,
        target,
        path,
        query: parse_query(&query_raw),
        headers,
        body: body.to_string(),
    })
}

fn split_header(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim().to_ascii_lowercase(), value.trim().to_string()))
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(hex);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

async fn write_response(stream: &mut TcpStream, spec: ResponseSpec) -> Result<(), String> {
    let reason = match spec.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        504 => "Gateway Timeout",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        spec.status,
        reason,
        spec.content_type,
        spec.body.len(),
        spec.body
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|err| err.to_string())
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn str_param(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn required_str(params: &Value, key: &str) -> Result<String, String> {
    str_param(params, key).ok_or_else(|| format!("缺少 {key}"))
}
