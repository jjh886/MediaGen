use serde_json::Value;

use crate::BrowserGatewayError;

pub(crate) fn runtime_result_to_value(result: &Value) -> Result<Value, BrowserGatewayError> {
    if let Some(exception) = result.get("exceptionDetails") {
        return Err(BrowserGatewayError::Cdp(exception.to_string()));
    }
    let value = result
        .get("result")
        .and_then(|remote| remote.get("value"))
        .cloned()
        .unwrap_or(Value::Null);
    if let Some(text) = value.as_str() {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            return Ok(parsed);
        }
    }
    Ok(value)
}

pub(crate) fn point_from_value(value: &Value) -> Option<(f64, f64)> {
    Some((value.get("x")?.as_f64()?, value.get("y")?.as_f64()?))
}

pub(crate) fn element_point_script(selector: &str) -> String {
    format!(
        r#"(() => {{
  const el = document.querySelector({});
  if (!el) return null;
  const rect = el.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return null;
  return {{ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }};
}})()"#,
        js_string(selector)
    )
}

pub(crate) fn text_point_script(text: &str, tag: &str, exact: bool) -> String {
    format!(
        r#"(() => {{
  const needle = {};
  const exact = {};
  const nodes = Array.from(document.querySelectorAll({}));
  const target = nodes.find((node) => {{
    const value = String(node.innerText || node.textContent || '').replace(/\s+/g, ' ').trim();
    if (!value) return false;
    return exact ? value === needle : value.includes(needle);
  }});
  if (!target) return null;
  const rect = target.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return null;
  return {{ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }};
}})()"#,
        js_string(text),
        exact,
        js_string(if tag.trim().is_empty() { "*" } else { tag })
    )
}

pub(crate) fn focus_script(selector: &str, clear_first: bool) -> String {
    format!(
        r#"(() => {{
  const el = document.querySelector({});
  if (!el) return false;
  el.focus();
  if ({}) {{
    if ('value' in el) {{
      el.value = '';
      el.dispatchEvent(new Event('input', {{ bubbles: true }}));
    }} else {{
      el.textContent = '';
    }}
  }}
  return true;
}})()"#,
        js_string(selector),
        clear_first
    )
}

pub(crate) fn download_script(url: &str) -> String {
    format!(
        r#"(async () => {{
  const resp = await fetch({}, {{ credentials: 'include' }});
  if (!resp.ok) throw new Error(`download failed: HTTP ${{resp.status}}`);
  const blob = await resp.blob();
  const buffer = await blob.arrayBuffer();
  let binary = '';
  const bytes = new Uint8Array(buffer);
  for (let i = 0; i < bytes.length; i += 1) binary += String.fromCharCode(bytes[i]);
  const mime = blob.type || 'image/png';
  return JSON.stringify({{
    status: 'done',
    images: [{{ url: `data:${{mime}};base64,${{btoa(binary)}}` }}],
    source: 'browser_download'
  }});
}})()"#,
        js_string(url)
    )
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}
