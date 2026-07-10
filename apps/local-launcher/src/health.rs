use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

pub fn wait_for_health(port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if is_healthy(port) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Err(format!(
        "服务启动超时，请查看 logs/startup.log。端口: {port}"
    ))
}

pub fn is_healthy(port: u16) -> bool {
    health_body(port).is_some()
}

pub fn is_current_install(port: u16, install_root: &Path) -> bool {
    let Some(body) = health_body(port) else {
        return false;
    };
    install_root_from_health(&body).is_some_and(|root| paths_match(&root, install_root))
}

fn health_body(port: u16) -> Option<String> {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return None;
    };
    let request = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(request).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    if !response.starts_with("HTTP/1.1 200") {
        return None;
    }
    response.split("\r\n\r\n").nth(1).map(str::to_string)
}

fn install_root_from_health(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    value
        .get("install_root")
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

fn paths_match(left: &str, right: &Path) -> bool {
    let left = std::path::PathBuf::from(left);
    let right = right.to_path_buf();
    if left == right {
        return true;
    }
    let left = left.canonicalize().ok();
    let right = right.canonicalize().ok();
    left.is_some() && left == right
}

#[cfg(test)]
mod tests {
    use super::{install_root_from_health, paths_match};
    use std::path::Path;

    #[test]
    fn reads_install_root_from_health_json() {
        let body = r#"{"ok":true,"service":"mediagen-server","install_root":"/opt/mediagen"}"#;

        assert_eq!(
            install_root_from_health(body).as_deref(),
            Some("/opt/mediagen")
        );
    }

    #[test]
    fn invalid_health_json_is_not_reused() {
        assert!(install_root_from_health("Request did not match any route").is_none());
    }

    #[test]
    fn path_match_accepts_identical_paths() {
        assert!(paths_match("/opt/mediagen", Path::new("/opt/mediagen")));
    }
}
