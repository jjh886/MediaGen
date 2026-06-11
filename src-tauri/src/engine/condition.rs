use serde_json::Value;

/// 通用条件求值：支持 `path == 'value'` 和 `path != 'value'`。
pub(crate) fn evaluate_condition(data: &Value, condition: &str) -> bool {
    let Some((path, expected, negate)) = parse_condition(condition) else {
        return false;
    };
    let actual = resolve_dot_path(data, path);
    let matches = value_as_text(actual) == expected;
    if negate {
        !matches
    } else {
        matches
    }
}

fn parse_condition(condition: &str) -> Option<(&str, &str, bool)> {
    if let Some((left, right)) = condition.split_once("!=") {
        return Some((left.trim(), trim_expected(right), true));
    }
    condition
        .split_once("==")
        .map(|(left, right)| (left.trim(), trim_expected(right), false))
}

fn trim_expected(value: &str) -> &str {
    value.trim().trim_matches('\'').trim_matches('"')
}

fn value_as_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.as_str().to_string(),
        Value::Null => String::new(),
        other => other.to_string().trim_matches('"').to_string(),
    }
}

fn resolve_dot_path<'a>(data: &'a Value, path: &str) -> &'a Value {
    let mut current = data;
    for key in path.split('.') {
        match current.get(key) {
            Some(value) => current = value,
            None => return &Value::Null,
        }
    }
    current
}
