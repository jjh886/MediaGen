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
    pub default_width: u32,
    pub default_height: u32,
    pub launch_browser: bool,
}

impl Default for BrowserGatewayConfig {
    fn default() -> Self {
        Self {
            profile_root: PathBuf::from("data/profiles"),
            browser_path: std::env::var_os("CLOAK_BROWSER_PATH").map(PathBuf::from),
            default_width: DEFAULT_WIDTH,
            default_height: DEFAULT_HEIGHT,
            launch_browser: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSessionPurpose {
    AddAccount,
    ReloginAccount,
    HumanVerification,
    TaskTakeover,
    DebugPreview,
}

impl Default for BrowserSessionPurpose {
    fn default() -> Self {
        Self::DebugPreview
    }
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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserInputRequest {
    Mouse(BrowserMouseInput),
    Wheel(BrowserWheelInput),
    Key(BrowserKeyInput),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserMouseInput {
    pub event: BrowserMouseEvent,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub button: BrowserMouseButton,
    #[serde(default)]
    pub buttons: u8,
    #[serde(default, rename = "clickCount", alias = "click_count")]
    pub click_count: u8,
    #[serde(default)]
    pub modifiers: u8,
    #[serde(default = "default_true", rename = "readCursor", alias = "read_cursor")]
    pub read_cursor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserWheelInput {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default, rename = "deltaX", alias = "delta_x")]
    pub delta_x: f64,
    #[serde(default, rename = "deltaY", alias = "delta_y")]
    pub delta_y: f64,
    #[serde(default)]
    pub modifiers: u8,
    #[serde(default = "default_true", rename = "readCursor", alias = "read_cursor")]
    pub read_cursor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserKeyInput {
    pub event: BrowserKeyEvent,
    pub key: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub modifiers: u8,
    #[serde(default, rename = "ctrlKey", alias = "ctrl_key")]
    pub ctrl_key: bool,
    #[serde(default, rename = "metaKey", alias = "meta_key")]
    pub meta_key: bool,
    #[serde(default, rename = "altKey", alias = "alt_key")]
    pub alt_key: bool,
    #[serde(default, rename = "shiftKey", alias = "shift_key")]
    pub shift_key: bool,
}

impl BrowserInputRequest {
    pub(crate) fn should_read_cursor(&self) -> bool {
        match self {
            Self::Mouse(input) => input.read_cursor,
            Self::Wheel(input) => input.read_cursor,
            Self::Key(_) => false,
        }
    }

    pub(crate) fn should_emit_event(&self) -> bool {
        !matches!(
            self,
            Self::Mouse(BrowserMouseInput {
                event: BrowserMouseEvent::Move,
                ..
            }) | Self::Wheel(_)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserMouseEvent {
    Move,
    Down,
    Up,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserMouseButton {
    #[default]
    Left,
    Middle,
    Right,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserKeyEvent {
    Down,
    Up,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserInputResponse {
    pub ok: bool,
    pub cursor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserClipboardRequest {
    #[serde(default)]
    pub action: BrowserClipboardAction,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserClipboardAction {
    Read,
    #[default]
    Write,
    CopySelection,
    InsertText,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserClipboardResponse {
    pub ok: bool,
    pub text: String,
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

fn default_true() -> bool {
    true
}
