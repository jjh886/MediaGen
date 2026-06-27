use serde_json::Value;

#[derive(Debug, Clone)]
pub(crate) struct RetryConfig {
    attempts: usize,
    delays_ms: Vec<u64>,
}

impl RetryConfig {
    pub(crate) fn from_params(params: &Value, attempts: usize, delays_ms: &[u64]) -> Self {
        let retry = params.get("retry").unwrap_or(&Value::Null);
        let attempts = retry
            .get("attempts")
            .and_then(|v| v.as_u64())
            .map(|v| v.clamp(1, 8) as usize)
            .unwrap_or(attempts.max(1));
        let delays = retry
            .get("delays_ms")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_u64())
                    .map(|v| v.clamp(250, 120_000))
                    .collect::<Vec<_>>()
            })
            .filter(|items| !items.is_empty())
            .unwrap_or_else(|| delays_ms.to_vec());
        Self {
            attempts,
            delays_ms: delays,
        }
    }

    pub(crate) fn attempts(&self) -> usize {
        self.attempts
    }

    pub(crate) fn delay_for_attempt(&self, attempt: usize) -> u64 {
        let index = attempt
            .saturating_sub(1)
            .min(self.delays_ms.len().saturating_sub(1));
        self.delays_ms.get(index).copied().unwrap_or(1_000)
    }
}

pub(crate) async fn sleep_async(config: &RetryConfig, attempt: usize) {
    tokio::time::sleep(std::time::Duration::from_millis(jitter_ms(
        config.delay_for_attempt(attempt),
        20,
    )))
    .await;
}

pub(crate) fn sleep_blocking(config: &RetryConfig, attempt: usize) {
    std::thread::sleep(std::time::Duration::from_millis(jitter_ms(
        config.delay_for_attempt(attempt),
        20,
    )));
}

pub(crate) fn is_transient_network_error(error: &str) -> bool {
    let text = error.to_ascii_lowercase();
    [
        "err_connection_closed",
        "err_connection_reset",
        "err_connection_refused",
        "err_network_changed",
        "err_internet_disconnected",
        "err_timed_out",
        "err_name_not_resolved",
        "err_address_unreachable",
        "err_proxy_connection_failed",
        "err_tunnel_connection_failed",
        "connection closed",
        "connection reset",
        "connection refused",
        "connection aborted",
        "unexpected eof",
        "incomplete message",
        "timed out",
        "timeout",
        "dns",
        "proxy",
        "tunnel",
        "network",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

pub(crate) fn is_retryable_http_status(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}

fn jitter_ms(base_ms: u64, pct: u64) -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let range = base_ms * pct / 100;
    if range == 0 {
        return base_ms.max(250);
    }
    let offset = nanos % (range * 2);
    (base_ms + offset - range).max(250)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_browser_network_errors() {
        assert!(is_transient_network_error(
            "Navigate failed: net::ERR_TUNNEL_CONNECTION_FAILED"
        ));
        assert!(is_transient_network_error("connection reset by peer"));
        assert!(!is_transient_network_error(
            "Cannot navigate to invalid URL"
        ));
    }
}
