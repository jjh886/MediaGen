use serde::Deserialize;

use super::SAFE_LAUNCH_URL;
use crate::BrowserGatewayError;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BrowserTarget {
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub websocket_url: String,
}

pub(crate) async fn get_best_page_target(
    debugging_port: u16,
) -> Result<BrowserTarget, BrowserGatewayError> {
    let targets = get_page_targets(debugging_port).await?;
    best_page_target(&targets)
}

pub(crate) async fn get_page_target(
    debugging_port: u16,
    target_id: &str,
) -> Result<Option<BrowserTarget>, BrowserGatewayError> {
    Ok(get_page_targets(debugging_port)
        .await?
        .into_iter()
        .find(|target| target.id == target_id))
}

pub(crate) async fn create_page_target(
    debugging_port: u16,
) -> Result<BrowserTarget, BrowserGatewayError> {
    reqwest::Client::new()
        .put(format!(
            "http://127.0.0.1:{debugging_port}/json/new?{SAFE_LAUNCH_URL}"
        ))
        .send()
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
        .error_for_status()
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
        .json::<BrowserTarget>()
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))
}

pub(crate) async fn close_page_target(
    debugging_port: u16,
    target_id: &str,
) -> Result<(), BrowserGatewayError> {
    if target_id.trim().is_empty() {
        return Ok(());
    }
    reqwest::get(format!(
        "http://127.0.0.1:{debugging_port}/json/close/{target_id}"
    ))
    .await
    .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
    .error_for_status()
    .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?;
    Ok(())
}

async fn get_page_targets(debugging_port: u16) -> Result<Vec<BrowserTarget>, BrowserGatewayError> {
    let url = format!("http://127.0.0.1:{debugging_port}/json/list");
    reqwest::get(url)
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))?
        .json::<Vec<BrowserTarget>>()
        .await
        .map_err(|err| BrowserGatewayError::Cdp(err.to_string()))
}

fn best_page_target(targets: &[BrowserTarget]) -> Result<BrowserTarget, BrowserGatewayError> {
    targets
        .iter()
        .find(|target| is_useful_page_url(&target.url))
        .or_else(|| targets.first())
        .cloned()
        .ok_or_else(|| BrowserGatewayError::Cdp("没有找到可控制的浏览器页面".to_string()))
}

pub(crate) fn is_useful_page_url(url: &str) -> bool {
    !url.is_empty() && url != "about:blank" && !url.starts_with("chrome://")
}
