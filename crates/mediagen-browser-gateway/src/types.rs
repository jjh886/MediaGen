use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub(crate) const DEFAULT_URL: &str = "https://example.com";
pub(crate) const DEFAULT_RUNTIME_ID: &str = "cloakbrowser";
pub(crate) const DEFAULT_BROWSER_NAME: &str = "CloakBrowser";
pub(crate) const DEFAULT_WIDTH: u32 = 1366;
pub(crate) const DEFAULT_HEIGHT: u32 = 820;
pub(crate) const DEFAULT_DEVICE_SCALE_FACTOR: f64 = 1.0;

#[derive(Debug, Error)]
pub enum BrowserGatewayError {
    #[error("browser session not found: {0}")]
    SessionNotFound(String),
    #[error("browser session is closed: {0}")]
    SessionClosed(String),
    #[error("browser runtime unavailable: {0}")]
    BrowserRuntimeUnavailable(String),
    #[error("browser launch failed: {0}")]
    BrowserLaunch(String),
    #[error("cdp error: {0}")]
    Cdp(String),
    #[error("browser gateway state is poisoned")]
    StatePoisoned,
}

#[derive(Debug, Clone)]
pub struct BrowserGatewayConfig {
    pub profile_root: PathBuf,
    pub browser_path: Option<PathBuf>,
    pub default_proxy_server: Option<String>,
    pub default_width: u32,
    pub default_height: u32,
    pub launch_browser: bool,
}

impl Default for BrowserGatewayConfig {
    fn default() -> Self {
        Self {
            profile_root: PathBuf::from("data/profiles"),
            browser_path: browser_path_from_env(),
            default_proxy_server: proxy_server_from_env(),
            default_width: DEFAULT_WIDTH,
            default_height: DEFAULT_HEIGHT,
            launch_browser: true,
        }
    }
}

fn browser_path_from_env() -> Option<PathBuf> {
    [
        "CLOAK_BROWSER_PATH",
        "CLOAKBROWSER_BINARY_PATH",
        "MEDIAGEN_CLOAK_BROWSER_DIR",
        "CLOAK_BROWSER_BUNDLE_DIR",
    ]
    .iter()
    .find_map(|name| std::env::var_os(name).map(PathBuf::from))
}

fn proxy_server_from_env() -> Option<String> {
    [
        "MEDIAGEN_BROWSER_PROXY",
        "CLOAK_BROWSER_PROXY",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .find_map(|name| std::env::var(name).ok())
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSessionPurpose {
    AddAccount,
    ReloginAccount,
    HumanVerification,
    TaskTakeover,
    #[default]
    DebugPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSessionStatus {
    Active,
    Completed,
    Cancelled,
    Failed,
}

impl BrowserSessionStatus {
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserViewport {
    pub width: u32,
    pub height: u32,
}

impl BrowserViewport {
    pub(crate) fn clamped(self) -> Self {
        Self {
            width: self.width.clamp(480, 2560),
            height: self.height.clamp(360, 1800),
        }
    }
}

impl Default for BrowserViewport {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrowserSessionCreateRequest {
    #[serde(default)]
    pub purpose: BrowserSessionPurpose,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub start_url: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub initial_url: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub proxy_id: Option<String>,
    #[serde(default)]
    pub verification_reason: Option<String>,
    #[serde(default)]
    pub runtime_id: Option<String>,
    #[serde(default)]
    pub profile_dir: Option<String>,
    #[serde(default)]
    pub browser_profile_dir: Option<String>,
    #[serde(default)]
    pub reuse_browser_profile_account_id: Option<String>,
    #[serde(default)]
    pub headless: Option<bool>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default, rename = "deviceScaleFactor", alias = "device_scale_factor")]
    pub device_scale_factor: Option<f64>,
    #[serde(default)]
    pub viewport: Option<BrowserViewport>,
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
}

impl BrowserSessionCreateRequest {
    pub(crate) fn resolved_url(&self) -> String {
        self.initial_url
            .as_deref()
            .or(self.url.as_deref())
            .or(Some(self.start_url.as_str()))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_URL)
            .to_string()
    }

    pub(crate) fn resolved_platform(&self) -> String {
        let platform = self.platform.trim();
        if platform.is_empty() {
            "browser".to_string()
        } else {
            platform.to_string()
        }
    }

    pub(crate) fn resolved_runtime_id(&self) -> String {
        self.runtime_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_RUNTIME_ID)
            .to_string()
    }

    pub(crate) fn resolved_viewport(&self, config: &BrowserGatewayConfig) -> BrowserViewport {
        self.viewport
            .unwrap_or(BrowserViewport {
                width: self.width.unwrap_or(config.default_width),
                height: self.height.unwrap_or(config.default_height),
            })
            .clamped()
    }

    pub(crate) fn resolved_device_scale_factor(&self) -> f64 {
        crate::rendering::normalize_device_scale_factor(self.device_scale_factor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSession {
    pub id: String,
    pub purpose: BrowserSessionPurpose,
    pub status: BrowserSessionStatus,
    pub platform: String,
    pub account_id: Option<String>,
    pub task_id: Option<String>,
    pub proxy_id: Option<String>,
    pub start_url: String,
    pub url: String,
    pub title: String,
    pub target_id: String,
    pub headless: bool,
    pub browser_name: String,
    pub runtime_id: String,
    pub profile_dir: String,
    pub browser_profile_dir: String,
    pub viewport: BrowserViewport,
    pub device_scale_factor: f64,
    pub message: String,
    pub cookie_count: usize,
    pub verification_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSessionResponse {
    pub ok: bool,
    pub session: BrowserSession,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserResizeRequest {
    pub width: u32,
    pub height: u32,
    #[serde(default, rename = "deviceScaleFactor", alias = "device_scale_factor")]
    pub device_scale_factor: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserResizeResponse {
    pub ok: bool,
    pub viewport: BrowserViewport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserNavigateRequest {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserNavigateResponse {
    pub ok: bool,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserHistoryRequest {
    pub action: BrowserHistoryAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserHistoryAction {
    Back,
    Forward,
    Reload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserHistoryResponse {
    pub ok: bool,
    pub action: BrowserHistoryAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserScreenshotRequest {
    #[serde(default = "default_screenshot_format")]
    pub format: String,
    #[serde(default)]
    pub quality: Option<u8>,
    #[serde(default)]
    pub full_page: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserScreenshotResponse {
    pub ok: bool,
    pub format: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserCdpRequest {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserCdpResponse {
    pub ok: bool,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSessionEvent {
    pub sequence: u64,
    pub event: String,
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

fn default_screenshot_format() -> String {
    "jpeg".to_string()
}
