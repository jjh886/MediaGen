use std::sync::Arc;

use serde_json::json;

use crate::cdp::CdpClient;
use crate::pointer_state::read_pointer_state;
use crate::{
    BrowserGateway, BrowserGatewayError, BrowserImeEvent, BrowserImeInput, BrowserInputRequest,
    BrowserInputResponse, BrowserKeyEvent, BrowserKeyInput, BrowserMouseButton, BrowserMouseEvent,
    BrowserMouseInput, BrowserPointerState, BrowserWheelInput,
};

impl BrowserGateway {
    pub async fn dispatch_input(
        &self,
        id: &str,
        request: BrowserInputRequest,
    ) -> Result<BrowserInputResponse, BrowserGatewayError> {
        let client = self.session_client(id).await?;
        let read_cursor = request.should_read_cursor();
        let emit_event = request.should_emit_event();
        match request {
            BrowserInputRequest::Mouse(input) => dispatch_mouse_input(&client, input).await?,
            BrowserInputRequest::Wheel(input) => dispatch_wheel_input(&client, input).await?,
            BrowserInputRequest::Key(input) => dispatch_key_input(&client, input).await?,
            BrowserInputRequest::Ime(input) => dispatch_ime_input(&client, input).await?,
        }
        self.input_response(id, read_cursor, emit_event).await
    }

    async fn input_response(
        &self,
        id: &str,
        read_cursor: bool,
        emit_event: bool,
    ) -> Result<BrowserInputResponse, BrowserGatewayError> {
        if emit_event {
            self.push_event(id, "input", json!({ "session_id": id, "accepted": true }))?;
        }
        let pointer = if read_cursor {
            self.read_pointer_state(id).await.unwrap_or_default()
        } else {
            BrowserPointerState::default()
        };
        Ok(BrowserInputResponse {
            ok: true,
            cursor: pointer.cursor.clone(),
            pointer,
        })
    }

    pub async fn read_pointer_state(
        &self,
        id: &str,
    ) -> Result<BrowserPointerState, BrowserGatewayError> {
        let client = self.session_client(id).await?;
        read_pointer_state(&client).await
    }
}

async fn dispatch_mouse_input(
    client: &Arc<CdpClient>,
    input: BrowserMouseInput,
) -> Result<(), BrowserGatewayError> {
    let _ = client
        .send(
            "Runtime.evaluate",
            json!({
                "expression": format!("window.__mediagenMouseX={};window.__mediagenMouseY={};", input.x, input.y),
                "returnByValue": true,
            }),
        )
        .await;
    client
        .send(
            "Input.dispatchMouseEvent",
            json!({
                "type": mouse_event_type(&input.event),
                "x": input.x,
                "y": input.y,
                "button": mouse_button_name(&input.button),
                "buttons": input.buttons,
                "clickCount": input.click_count.max(1),
                "modifiers": input.modifiers,
            }),
        )
        .await?;
    Ok(())
}

async fn dispatch_wheel_input(
    client: &Arc<CdpClient>,
    input: BrowserWheelInput,
) -> Result<(), BrowserGatewayError> {
    client
        .send(
            "Input.dispatchMouseEvent",
            json!({
                "type": "mouseWheel",
                "x": input.x,
                "y": input.y,
                "deltaX": input.delta_x,
                "deltaY": input.delta_y,
                "modifiers": input.modifiers,
            }),
        )
        .await?;
    Ok(())
}

async fn dispatch_key_input(
    client: &Arc<CdpClient>,
    input: BrowserKeyInput,
) -> Result<(), BrowserGatewayError> {
    if input.event == BrowserKeyEvent::Down && is_printable_input(&input) {
        client
            .send("Input.insertText", json!({ "text": input.key }))
            .await?;
        return Ok(());
    }
    let key_code = key_code_for(&input.key);
    client
        .send(
            "Input.dispatchKeyEvent",
            json!({
                "type": key_event_type(&input.event),
                "key": input.key,
                "code": input.code,
                "windowsVirtualKeyCode": key_code,
                "nativeVirtualKeyCode": key_code,
                "modifiers": key_modifiers(&input),
                "text": input.text,
                "unmodifiedText": input.unmodified_text,
                "autoRepeat": input.auto_repeat,
                "isKeypad": input.is_keypad,
                "location": input.location,
                "commands": input.commands,
            }),
        )
        .await?;
    Ok(())
}

async fn dispatch_ime_input(
    client: &Arc<CdpClient>,
    input: BrowserImeInput,
) -> Result<(), BrowserGatewayError> {
    match input.event {
        BrowserImeEvent::SetComposition => {
            client
                .send(
                    "Input.imeSetComposition",
                    json!({
                        "text": input.text,
                        "selectionStart": input.selection_start,
                        "selectionEnd": input.selection_end,
                    }),
                )
                .await?;
        }
        BrowserImeEvent::Commit => {
            client
                .send("Input.insertText", json!({ "text": input.text }))
                .await?;
        }
        BrowserImeEvent::Cancel => {
            client
                .send(
                    "Input.imeSetComposition",
                    json!({ "text": "", "selectionStart": 0, "selectionEnd": 0 }),
                )
                .await?;
        }
    }
    Ok(())
}

fn mouse_event_type(event: &BrowserMouseEvent) -> &'static str {
    match event {
        BrowserMouseEvent::Move => "mouseMoved",
        BrowserMouseEvent::Down => "mousePressed",
        BrowserMouseEvent::Up => "mouseReleased",
    }
}

fn mouse_button_name(button: &BrowserMouseButton) -> &'static str {
    match button {
        BrowserMouseButton::Left => "left",
        BrowserMouseButton::Middle => "middle",
        BrowserMouseButton::Right => "right",
        BrowserMouseButton::None => "none",
    }
}

fn key_event_type(event: &BrowserKeyEvent) -> &'static str {
    match event {
        BrowserKeyEvent::Down => "rawKeyDown",
        BrowserKeyEvent::Up => "keyUp",
    }
}

fn key_code_for(key: &str) -> u32 {
    match key {
        "Backspace" => 8,
        "Tab" => 9,
        "Enter" => 13,
        "Escape" => 27,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Delete" => 46,
        _ if key.chars().count() == 1 => {
            key.to_ascii_uppercase().bytes().next().unwrap_or(0) as u32
        }
        _ => 0,
    }
}

fn is_printable_input(input: &BrowserKeyInput) -> bool {
    input.key.chars().count() == 1 && !input.ctrl_key && !input.meta_key && !input.alt_key
}

fn key_modifiers(input: &BrowserKeyInput) -> u8 {
    let mut modifiers = input.modifiers;
    if input.alt_key {
        modifiers |= 1;
    }
    if input.ctrl_key {
        modifiers |= 2;
    }
    if input.meta_key {
        modifiers |= 4;
    }
    if input.shift_key {
        modifiers |= 8;
    }
    modifiers
}
