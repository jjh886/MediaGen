use std::io::{Read, Write};
use std::net::TcpStream;
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
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let request = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    if stream.write_all(request).is_err() {
        return false;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).is_ok() && response.starts_with("HTTP/1.1 200")
}
