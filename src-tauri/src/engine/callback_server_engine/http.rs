use std::collections::HashMap;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::AppError;

pub(super) struct ResponseSpec {
    pub(super) status: u16,
    pub(super) content_type: String,
    pub(super) body: String,
}

pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) path: String,
    pub(super) query: HashMap<String, String>,
    pub(super) headers: HashMap<String, String>,
    pub(super) body: String,
}

pub(super) async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, AppError> {
    let mut buf = vec![0u8; 64 * 1024];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Err(AppError::PlatformError("callback request 为空".into()));
    }
    let raw = String::from_utf8_lossy(&buf[..n]).to_string();
    parse_http_request(&raw)
}

fn parse_http_request(raw: &str) -> Result<HttpRequest, AppError> {
    if raw.trim().is_empty() {
        return Err(AppError::PlatformError("callback request 为空".into()));
    }
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let mut lines = head.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| AppError::PlatformError("callback request 缺少请求行".into()))?;
    if request_line.trim().is_empty() {
        return Err(AppError::PlatformError("callback request 为空".into()));
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let (path, query_raw) = split_target(&target);
    let query = parse_query(query_raw);
    let headers = lines.filter_map(split_header).collect::<HashMap<_, _>>();
    Ok(HttpRequest {
        method,
        target,
        path,
        query,
        headers,
        body: body.to_string(),
    })
}

fn split_header(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim().to_ascii_lowercase(), value.trim().to_string()))
}

fn split_target(target: &str) -> (String, &str) {
    target
        .split_once('?')
        .map(|(path, query)| (path.to_string(), query))
        .unwrap_or_else(|| (target.to_string(), ""))
}

fn parse_query(query: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

pub(super) async fn write_json_response(
    stream: &mut TcpStream,
    status: u16,
    body: Value,
) -> Result<(), AppError> {
    write_response(
        stream,
        ResponseSpec {
            status,
            content_type: "application/json".to_string(),
            body: body.to_string(),
        },
    )
    .await
}

pub(super) async fn write_response(
    stream: &mut TcpStream,
    spec: ResponseSpec,
) -> Result<(), AppError> {
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
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_request_rejects_empty_input_as_empty_request() {
        let error = match parse_http_request("") {
            Ok(_) => panic!("empty request should fail"),
            Err(error) => error.to_string(),
        };
        assert_eq!(error, "平台错误: callback request 为空");
    }

    #[test]
    fn parse_http_request_rejects_blank_request_line_as_empty_request() {
        let error = match parse_http_request("\r\n\r\n") {
            Ok(_) => panic!("blank request should fail"),
            Err(error) => error.to_string(),
        };
        assert_eq!(error, "平台错误: callback request 为空");
    }
}
