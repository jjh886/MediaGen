use std::sync::Arc;

use serde_json::json;

use crate::cdp::CdpClient;
use crate::{BrowserGatewayError, BrowserSession, BrowserViewport};

const SCREENCAST_QUALITY: u8 = 84;

pub(crate) fn normalize_device_scale_factor(value: Option<f64>) -> f64 {
    let scale = value.unwrap_or(1.0);
    if scale.is_finite() {
        scale.clamp(1.0, 2.0)
    } else {
        1.0
    }
}

pub(crate) async fn configure_rendering(
    client: &Arc<CdpClient>,
    session: &BrowserSession,
) -> Result<(), BrowserGatewayError> {
    let _viewport = client.viewport_lock.lock().await;
    apply_viewport(client, session.viewport, session.device_scale_factor).await?;
    restart_screencast(client).await
}

pub(crate) async fn apply_viewport(
    client: &Arc<CdpClient>,
    viewport: BrowserViewport,
    device_scale_factor: f64,
) -> Result<(), BrowserGatewayError> {
    client
        .send(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": viewport.width,
                "height": viewport.height,
                "deviceScaleFactor": device_scale_factor,
                "mobile": false,
            }),
        )
        .await?;
    Ok(())
}

pub(crate) async fn restart_screencast(client: &Arc<CdpClient>) -> Result<(), BrowserGatewayError> {
    let _ = client.send("Page.stopScreencast", json!({})).await;
    client
        .send(
            "Page.startScreencast",
            json!({
                "format": "jpeg",
                "quality": SCREENCAST_QUALITY,
                "everyNthFrame": 1,
            }),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_device_scale_factor;

    #[test]
    fn device_scale_factor_is_safe_for_remote_rendering() {
        assert_eq!(normalize_device_scale_factor(None), 1.0);
        assert_eq!(normalize_device_scale_factor(Some(0.5)), 1.0);
        assert_eq!(normalize_device_scale_factor(Some(1.5)), 1.5);
        assert_eq!(normalize_device_scale_factor(Some(5.0)), 2.0);
        assert_eq!(normalize_device_scale_factor(Some(f64::NAN)), 1.0);
    }
}
