use std::sync::Arc;

use serde_json::{json, Value};

use crate::cdp::CdpClient;
use crate::{BrowserGatewayError, BrowserPointerState};

// 浏览器截图不携带系统光标，必须根据远端命中的 DOM 节点还原浏览器语义光标。
const POINTER_STATE_SCRIPT: &str = r#"
(() => {
  const x = Number(window.__mediagenMouseX || 0);
  const y = Number(window.__mediagenMouseY || 0);
  const hit = (doc, px, py) => {
    let element = doc.elementFromPoint(px, py);
    if (!element) return null;
    while (element?.shadowRoot) {
      const nested = element.shadowRoot.elementFromPoint(px, py);
      if (!nested || nested === element) break;
      element = nested;
    }
    if (element?.tagName === 'IFRAME') {
      try {
        const rect = element.getBoundingClientRect();
        const nested = hit(element.contentDocument, px - rect.left, py - rect.top);
        if (nested) return nested;
      } catch (_) {}
    }
    return { element, doc, x: px, y: py };
  };
  const target = hit(document, x, y);
  if (!target) return { cursor: 'default', element_kind: 'none' };
  const { element, doc, x: localX, y: localY } = target;
  const style = getComputedStyle(element);
  const tag = String(element.tagName || '').toLowerCase();
  const role = String(element.getAttribute?.('role') || '').toLowerCase();
  const type = String(element.getAttribute?.('type') || '').toLowerCase();
  const editable = element.isContentEditable || tag === 'textarea' ||
    (tag === 'input' && !['button', 'submit', 'reset', 'checkbox', 'radio', 'file', 'range', 'color'].includes(type));
  const password = tag === 'input' && type === 'password';
  const selectable = editable || style.userSelect !== 'none';
  const selection = String(doc.getSelection?.()?.toString() || '');
  const inputSelection = editable && Number(element.selectionEnd) > Number(element.selectionStart);
  const pointOverText = (() => {
    const range = doc.caretRangeFromPoint?.(localX, localY);
    const node = range?.startContainer;
    if (!node || node.nodeType !== Node.TEXT_NODE || !node.textContent) return false;
    const offset = Math.min(range.startOffset, Math.max(0, node.textContent.length - 1));
    const probe = doc.createRange();
    probe.setStart(node, offset);
    probe.setEnd(node, Math.min(node.textContent.length, offset + 1));
    const rect = probe.getBoundingClientRect();
    return localX >= rect.left - 2 && localX <= rect.right + 2 && localY >= rect.top && localY <= rect.bottom;
  })();
  let cursor = style.cursor || 'auto';
  if (cursor === 'auto') {
    if (editable || (selectable && pointOverText)) cursor = 'text';
    else if (tag === 'a' || role === 'link') cursor = 'pointer';
    else cursor = 'default';
  }
  return {
    cursor,
    editable,
    selectable,
    has_selection: Boolean(selection || inputSelection),
    password,
    element_kind: editable ? 'editable' : (tag === 'a' || role === 'link' ? 'link' : 'content')
  };
})()
"#;

pub(crate) async fn read_pointer_state(
    client: &Arc<CdpClient>,
) -> Result<BrowserPointerState, BrowserGatewayError> {
    let result = client
        .send(
            "Runtime.evaluate",
            json!({ "expression": POINTER_STATE_SCRIPT, "returnByValue": true }),
        )
        .await?;
    let value = result
        .get("result")
        .and_then(|item| item.get("value"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(pointer_state_from_value(&value))
}

fn pointer_state_from_value(value: &Value) -> BrowserPointerState {
    BrowserPointerState {
        cursor: string_field(value, "cursor", "default"),
        editable: bool_field(value, "editable"),
        selectable: bool_field(value, "selectable"),
        has_selection: bool_field(value, "has_selection"),
        password: bool_field(value, "password"),
        element_kind: string_field(value, "element_kind", "content"),
    }
}

fn string_field(value: &Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::pointer_state_from_value;
    use serde_json::json;

    #[test]
    fn pointer_state_keeps_semantic_flags() {
        let state = pointer_state_from_value(&json!({
            "cursor": "text",
            "editable": true,
            "selectable": true,
            "has_selection": true,
            "password": false,
            "element_kind": "editable"
        }));
        assert_eq!(state.cursor, "text");
        assert!(state.editable && state.selectable && state.has_selection);
    }
}
