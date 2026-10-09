//! 插件声明查询分组；宿主提供共享退避、串行窗口和短期同请求合并，不解释平台路由。
use super::ServerPluginHost;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

#[derive(Default)]
pub(super) struct Requests(Mutex<HashMap<String, Arc<Mutex<Window>>>>);
struct Window {
    next: Instant,
    limited: u32,
    cached: Option<(String, Instant, Value)>,
}
struct Control {
    group: String,
    interval: Duration,
    backoff: u64,
    maximum: u64,
    cache: Duration,
    wait: bool,
}
fn invalid() -> String {
    "HTTP 查询协调参数无效".into()
}
fn text(value: &Value, key: &str) -> Result<String, String> {
    value[key]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 160
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
        })
        .map(str::to_owned)
        .ok_or_else(invalid)
}
impl Control {
    fn parse(params: &Value, namespace: &str) -> Result<Option<Self>, String> {
        let Some(value) = params.get("request_control") else {
            return Ok(None);
        };
        if params["method"]
            .as_str()
            .is_some_and(|m| !m.eq_ignore_ascii_case("GET"))
        {
            return Err(invalid());
        }
        let number = |key: &str, default: u64, max: u64| -> Result<u64, String> {
            match value.get(key) {
                None => Ok(default),
                Some(v) => v.as_u64().filter(|n| *n <= max).ok_or_else(invalid),
            }
        };
        let group = text(value, "group")?;
        let identity = text(value, "identity")?;
        let url = reqwest::Url::parse(params["url"].as_str().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        Ok(Some(Self {
            group: format!(
                "{}:{}:{}:{}",
                namespace,
                url.origin().ascii_serialization(),
                identity,
                group
            ),
            interval: Duration::from_millis(number("min_interval_ms", 1000, 60000)?),
            backoff: number("rate_limit_delay_ms", 120000, 3600000)?.max(1000),
            maximum: number("rate_limit_max_delay_ms", 900000, 3600000)?.max(1000),
            cache: Duration::from_millis(number("cache_ms", 0, 60000)?),
            wait: value["wait"].as_bool().unwrap_or(true),
        }))
    }
}
fn limited_result(delay: Duration) -> Value {
    let seconds = delay.as_secs().saturating_add(1);
    json!({"status":429,"ok":false,"headers":{"retry-after":seconds.to_string()},"body":{"error":{"code":"client_rate_limited","message":"查询已按平台限流要求延后","retry_after_seconds":seconds}},"coalesced":true})
}
fn retry_after(value: &Value) -> Option<Duration> {
    let header = value["headers"]
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("retry-after"))?
        .1
        .as_str()?;
    if let Ok(seconds) = header.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds.min(86400)));
    }
    let date = chrono::DateTime::parse_from_rfc2822(header).ok()?;
    Some(Duration::from_millis(
        (date.timestamp_millis() - chrono::Utc::now().timestamp_millis()).max(0) as u64,
    ))
}
static REQUESTS: OnceLock<Requests> = OnceLock::new();

impl ServerPluginHost {
    pub(super) async fn controlled_request(&self, params: &Value) -> Result<Value, String> {
        let Some(control) = Control::parse(params, &self.profile_dir.to_string_lossy())? else {
            return self.http_request(params, false).await;
        };
        let slot = {
            let mut slots = REQUESTS.get_or_init(Requests::default).0.lock().await;
            if !slots.contains_key(&control.group) && slots.len() >= 128 {
                slots.retain(|_, slot| {
                    Arc::strong_count(slot) > 1
                        || slot.try_lock().map_or(true, |window| {
                            window.next > Instant::now()
                                || window
                                    .cached
                                    .as_ref()
                                    .is_some_and(|(_, expires, _)| *expires > Instant::now())
                        })
                });
                if slots.len() >= 128 {
                    return Err("HTTP 查询分组过多".into());
                }
            }
            slots
                .entry(control.group.clone())
                .or_insert_with(|| {
                    Arc::new(Mutex::new(Window {
                        next: Instant::now(),
                        limited: 0,
                        cached: None,
                    }))
                })
                .clone()
        };
        let mut window = if control.wait {
            slot.lock().await
        } else {
            match slot.try_lock() {
                Ok(guard) => guard,
                Err(_) => return Ok(limited_result(control.interval.max(Duration::from_secs(1)))),
            }
        };
        let vary = params["headers"].as_object().map(|headers| {
            headers
                .iter()
                .filter(|(key, _)| {
                    [
                        "authorization",
                        "cookie",
                        "accept",
                        "accept-language",
                        "user-agent",
                    ]
                    .contains(&key.to_ascii_lowercase().as_str())
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        });
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(
                json!([
                    params["url"],
                    vary,
                    params["cookies"],
                    params["response_body_format"]
                ])
                .to_string()
                .as_bytes()
            )
        );
        if let Some((key, expires, value)) = &window.cached {
            if key == &fingerprint && *expires > Instant::now() {
                return Ok(value.clone());
            }
        }
        let delay = window.next.saturating_duration_since(Instant::now());
        if !delay.is_zero() {
            if !control.wait {
                return Ok(limited_result(delay));
            }
            tokio::time::sleep(delay).await;
        }
        window.next = Instant::now() + control.interval;
        let result = self.http_request(params, false).await?;
        if result["status"] == 429 {
            window.limited = window.limited.saturating_add(1).min(16);
            let backoff = control
                .backoff
                .saturating_mul(1u64 << (window.limited - 1))
                .min(control.maximum);
            let delay = retry_after(&result)
                .unwrap_or_default()
                .max(Duration::from_millis(backoff));
            window.next = Instant::now() + delay;
            window.cached = None;
        } else if result["ok"] == true {
            window.limited = 0;
            window.next = Instant::now() + control.interval;
            if !control.cache.is_zero() && result.to_string().len() <= 1 << 20 {
                window.cached = Some((fingerprint, Instant::now() + control.cache, result.clone()));
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn server(status: u16) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                seen.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut request = [0; 4096];
                    assert!(socket.read(&mut request).await.unwrap() > 0);
                    let body = "{\"value\":\"result\"}";
                    let response = format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nRetry-After: 180\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        (url, count, task)
    }
    fn params(url: &str) -> Value {
        json!({"url":url,"method":"GET","headers":{"authorization":"Bearer session","oai-session-id":"volatile"},"retry":{"max_attempts":1},"request_control":{"group":"queries","identity":"account","min_interval_ms":5000,"cache_ms":3000,"wait":false}})
    }
    #[tokio::test]
    async fn one_rate_limit_pauses_other_queries_in_the_same_account_without_sending() {
        let (url, count, server) = server(429).await;
        let engine = ServerPluginHost::new_with_metadata(String::new(), &HashMap::new());
        let first = engine
            .execute("http.request", &params(&format!("{url}/first")))
            .await
            .unwrap();
        assert_eq!(first["status"], 429);
        for index in 0..34 {
            let result = engine
                .execute("http.request", &params(&format!("{url}/other-{index}")))
                .await
                .unwrap();
            assert_eq!(result["status"], 429);
            assert_eq!(result["coalesced"], true);
            assert!(
                result["headers"]["retry-after"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    >= 179
            );
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let mut other = params(&format!("{url}/other-account"));
        other["request_control"]["identity"] = json!("another-account");
        engine.execute("http.request", &other).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        server.abort();
    }
    #[tokio::test]
    async fn duplicate_queries_share_cache_despite_volatile_headers_but_not_credentials() {
        let (url, count, server) = server(200).await;
        let engine = ServerPluginHost::new_with_metadata(String::new(), &HashMap::new());
        let mut request = params(&format!("{url}/conversation"));
        request["request_control"]["min_interval_ms"] = json!(0);
        let first = engine.execute("http.request", &request).await.unwrap();
        request["headers"]["oai-session-id"] = json!("another-runtime");
        let second = engine.execute("http.request", &request).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        request["headers"]["authorization"] = json!("Bearer different-session");
        engine.execute("http.request", &request).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        server.abort();
    }
    #[test]
    fn mutations_and_oversized_groups_cannot_use_query_control() {
        let mut request = params("https://example.com/");
        request["method"] = json!("POST");
        assert!(Control::parse(&request, "test").is_err());
        request["method"] = json!("GET");
        request["request_control"]["identity"] = json!("a".repeat(161));
        assert!(Control::parse(&request, "test").is_err());
    }
}
