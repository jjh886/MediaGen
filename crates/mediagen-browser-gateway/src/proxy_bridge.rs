use crate::{BrowserGatewayError, BrowserSession};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

type Bridges = Mutex<HashMap<String, Weak<Bridge>>>;
static BRIDGES: OnceLock<Bridges> = OnceLock::new();
pub(crate) struct Bridge {
    pub address: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[derive(Clone)]
pub(crate) struct Config {
    host: String,
    port: u16,
    user: String,
    password: String,
}
fn decode(value: &str) -> String {
    // URL 表单解析器处理百分号转义；先保留凭证中原本的加号。
    reqwest::Url::parse(&format!("http://local/?v={}", value.replace('+', "%2B")))
        .ok()
        .and_then(|url| url.query_pairs().next().map(|(_, v)| v.into_owned()))
        .unwrap_or_default()
}
pub(crate) fn config(session: &BrowserSession) -> Option<Config> {
    let raw = session.metadata.get("proxy_server")?.as_str()?;
    let url = reqwest::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "socks5" | "socks5h") {
        return None;
    }
    let text = |key: &str| {
        session
            .metadata
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    let user = text("proxy_username").unwrap_or_else(|| decode(url.username()));
    if user.is_empty() {
        return None;
    }
    Some(Config {
        host: url.host_str()?.into(),
        port: url.port()?,
        user,
        password: text("proxy_password").unwrap_or_else(|| decode(url.password().unwrap_or(""))),
    })
}
pub(crate) fn existing(profile: &str) -> Option<Arc<Bridge>> {
    BRIDGES
        .get_or_init(Mutex::default)
        .lock()
        .ok()?
        .get(profile)?
        .upgrade()
}
pub(crate) async fn start(
    profile: &str,
    config: Config,
) -> Result<Arc<Bridge>, BrowserGatewayError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| BrowserGatewayError::BrowserLaunch("创建代理转接失败".into()))?;
    let address = format!(
        "http://{}",
        listener
            .local_addr()
            .map_err(|_| BrowserGatewayError::BrowserLaunch("获取代理监听地址失败".into()))?
    );
    let config = Arc::new(config);
    let slots = Arc::new(tokio::sync::Semaphore::new(128));
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let config = config.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(3600),
                    relay(&mut socket, &config),
                )
                .await;
                if !matches!(result, Ok(Ok(()))) {
                    let _=socket.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                }
            });
        }
    });
    let bridge = Arc::new(Bridge { address, task });
    BRIDGES
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| BrowserGatewayError::StatePoisoned)?
        .insert(profile.into(), Arc::downgrade(&bridge));
    Ok(bridge)
}
async fn relay(
    client: &mut TcpStream,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut header = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while !header.ends_with(b"\r\n\r\n") {
            if header.len() >= 16384 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "header too large",
                ));
            }
            header.push(client.read_u8().await?);
        }
        Ok::<_, std::io::Error>(())
    })
    .await??;
    let request = std::str::from_utf8(&header)?;
    let mut lines = request.split("\r\n");
    let first = lines.next().ok_or("missing request")?;
    let mut parts = first.split_whitespace();
    let method = parts.next().ok_or("missing method")?;
    let target = parts.next().ok_or("missing target")?;
    let url = reqwest::Url::parse(&if method == "CONNECT" {
        format!("https://{target}")
    } else {
        target.to_string()
    })?;
    let host = url.host_str().ok_or("missing host")?;
    let port = url.port_or_known_default().ok_or("missing port")?;
    let mut upstream = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio_socks::tcp::Socks5Stream::connect_with_password(
            (config.host.as_str(), config.port),
            (host, port),
            &config.user,
            &config.password,
        ),
    )
    .await??
    .into_inner();
    if method == "CONNECT" {
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
    } else {
        if url.scheme() != "http" {
            return Err("invalid proxy request".into());
        }
        let mut rewritten = format!(
            "{method} {}{} HTTP/1.1\r\n",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        for line in lines.filter(|line| !line.is_empty()) {
            let name = line.split(':').next().unwrap_or("");
            if !["proxy-authorization", "proxy-connection", "connection"]
                .iter()
                .any(|v| name.eq_ignore_ascii_case(v))
            {
                rewritten.push_str(line);
                rewritten.push_str("\r\n");
            }
        }
        rewritten.push_str("Connection: close\r\n\r\n");
        upstream.write_all(rewritten.as_bytes()).await?;
    }
    tokio::io::copy_bidirectional(client, &mut upstream).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn authenticated_socks_tunnel_preserves_remote_dns_and_binary_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            assert_eq!(socket.read_u8().await.unwrap(), 5);
            let count = socket.read_u8().await.unwrap();
            let mut methods = vec![0; count as usize];
            socket.read_exact(&mut methods).await.unwrap();
            assert!(methods.contains(&2));
            socket.write_all(&[5, 2]).await.unwrap();
            assert_eq!(socket.read_u8().await.unwrap(), 1);
            let length = socket.read_u8().await.unwrap();
            let mut user = vec![0; length as usize];
            socket.read_exact(&mut user).await.unwrap();
            assert_eq!(user, b"test-user");
            let length = socket.read_u8().await.unwrap();
            let mut password = vec![0; length as usize];
            socket.read_exact(&mut password).await.unwrap();
            assert_eq!(password, b"test-password");
            socket.write_all(&[1, 0]).await.unwrap();
            let mut prefix = [0; 4];
            socket.read_exact(&mut prefix).await.unwrap();
            assert_eq!(prefix, [5, 1, 0, 3]);
            let length = socket.read_u8().await.unwrap();
            let mut host = vec![0; length as usize];
            socket.read_exact(&mut host).await.unwrap();
            assert_eq!(host, b"target.invalid");
            assert_eq!(socket.read_u16().await.unwrap(), 443);
            socket
                .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 1, 1])
                .await
                .unwrap();
            let mut data = [0; 4];
            socket.read_exact(&mut data).await.unwrap();
            assert_eq!(data, [0, 1, 254, 255]);
            socket.write_all(&data).await.unwrap();
        });
        let bridge = start(
            "test-profile",
            Config {
                host: "127.0.0.1".into(),
                port,
                user: "test-user".into(),
                password: "test-password".into(),
            },
        )
        .await
        .unwrap();
        let mut client = TcpStream::connect(bridge.address.trim_start_matches("http://"))
            .await
            .unwrap();
        client
            .write_all(b"CONNECT target.invalid:443 HTTP/1.1\r\nHost: target.invalid:443\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            response.push(client.read_u8().await.unwrap());
        }
        assert!(String::from_utf8(response)
            .unwrap()
            .contains("200 Connection Established"));
        client.write_all(&[0, 1, 254, 255]).await.unwrap();
        let mut data = [0; 4];
        client.read_exact(&mut data).await.unwrap();
        assert_eq!(data, [0, 1, 254, 255]);
        server.await.unwrap();
    }
    #[test]
    fn percent_decoding_preserves_special_characters() {
        assert_eq!(
            decode("user%40example%3Apass%2Fword%26x+y"),
            "user@example:pass/word&x+y"
        );
    }
}
