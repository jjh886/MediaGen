use serde_json::json;

use crate::{
    BrowserGateway, BrowserGatewayError, BrowserScreenshotRequest, BrowserScreenshotResponse,
};

impl BrowserGateway {
    pub async fn screenshot_session(
        &self,
        id: &str,
        request: BrowserScreenshotRequest,
    ) -> Result<BrowserScreenshotResponse, BrowserGatewayError> {
        let format = normalize_format(&request.format);
        let mut params = json!({
            "format": format,
            "fromSurface": true,
            "captureBeyondViewport": request.full_page,
        });
        if let Some(quality) = request.quality.filter(|_| format != "png") {
            params["quality"] = json!(quality.clamp(1, 100));
        }
        let result = self
            .session_client(id)
            .await?
            .send("Page.captureScreenshot", params)
            .await?;
        let data = result["data"]
            .as_str()
            .ok_or_else(|| BrowserGatewayError::Cdp("截图结果缺少 data".to_string()))?
            .to_string();
        Ok(BrowserScreenshotResponse {
            ok: true,
            format: format.to_string(),
            data,
        })
    }
}

fn normalize_format(format: &str) -> &'static str {
    match format.trim().to_ascii_lowercase().as_str() {
        "png" => "png",
        "webp" => "webp",
        _ => "jpeg",
    }
}
