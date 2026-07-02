use serde_json::Value;

pub(crate) fn collect_script_files(params: &Value) -> Vec<String> {
    if let Some(items) = params.get("script_files").and_then(Value::as_array) {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
    }
    params
        .get("script_file")
        .and_then(Value::as_str)
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}

pub(crate) fn should_skip(params: &Value) -> bool {
    truthy(params.get("skip_if"))
        || (truthy(params.get("skip_if_empty"))
            && ["selector", "text", "url"]
                .iter()
                .any(|key| empty_like(params.get(*key))))
}

pub(crate) fn resolve_templates(value: &Value, context: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(replace_template_vars(value, context)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| resolve_templates(item, context))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), resolve_templates(value, context)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => matches!(value.trim(), "true" | "1" | "yes"),
        Some(Value::Number(value)) => value.as_i64().unwrap_or_default() != 0,
        _ => false,
    }
}

fn empty_like(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(value)) => value.trim().is_empty() || value.contains("{{"),
        Some(Value::Array(value)) => value.is_empty(),
        Some(Value::Null) | None => true,
        _ => false,
    }
}

fn replace_template_vars(input: &str, context: &Value) -> String {
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let expr = after[..end].trim();
        let (path, js_escape) = expr
            .strip_prefix("js:")
            .map_or((expr, false), |path| (path.trim(), true));
        if let Some(value) = resolve_path(context, path) {
            let rendered = if js_escape { escape_js(&value) } else { value };
            out.push_str(&rendered);
        } else {
            out.push_str(&rest[start..start + end + 4]);
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

fn resolve_path(context: &Value, path: &str) -> Option<String> {
    let mut current = context;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    match current {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null => Some("null".to_string()),
        other => Some(other.to_string()),
    }
}

fn escape_js(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
