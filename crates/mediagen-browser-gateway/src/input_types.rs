use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserInputRequest {
    Mouse(BrowserMouseInput),
    Wheel(BrowserWheelInput),
    Key(BrowserKeyInput),
    Ime(BrowserImeInput),
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
    #[serde(default)]
    pub text: String,
    #[serde(default, rename = "unmodifiedText", alias = "unmodified_text")]
    pub unmodified_text: String,
    #[serde(default, rename = "autoRepeat", alias = "auto_repeat")]
    pub auto_repeat: bool,
    #[serde(default, rename = "isKeypad", alias = "is_keypad")]
    pub is_keypad: bool,
    #[serde(default)]
    pub location: u8,
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserImeInput {
    pub event: BrowserImeEvent,
    #[serde(default)]
    pub text: String,
    #[serde(default, rename = "selectionStart", alias = "selection_start")]
    pub selection_start: i32,
    #[serde(default, rename = "selectionEnd", alias = "selection_end")]
    pub selection_end: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserImeEvent {
    SetComposition,
    Commit,
    Cancel,
}

impl BrowserInputRequest {
    pub(crate) fn should_read_cursor(&self) -> bool {
        match self {
            Self::Mouse(input) => input.read_cursor,
            Self::Wheel(input) => input.read_cursor,
            Self::Key(_) | Self::Ime(_) => false,
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
    #[serde(default)]
    pub pointer: BrowserPointerState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserPointerState {
    pub cursor: String,
    pub editable: bool,
    pub selectable: bool,
    pub has_selection: bool,
    pub password: bool,
    pub element_kind: String,
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
    CutSelection,
    SelectAll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserClipboardResponse {
    pub ok: bool,
    pub text: String,
}

fn default_true() -> bool {
    true
}
