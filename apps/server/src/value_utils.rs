use serde_json::Value;

pub fn file_paths_from_value(value: &Value) -> Vec<String> {
    if let Some(items) = value.as_array() {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
    }
    if let Some(text) = value.as_str() {
        return file_paths_from_text(text);
    }
    Vec::new()
}

fn file_paths_from_text(text: &str) -> Vec<String> {
    if let Ok(items) = serde_json::from_str::<Vec<String>>(text) {
        return items;
    }
    if text.trim().is_empty() {
        Vec::new()
    } else {
        vec![text.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_paths_from_array_and_json_string() {
        assert_eq!(
            file_paths_from_value(&json!(["/tmp/a.png", "/tmp/b.png", 12])),
            vec!["/tmp/a.png", "/tmp/b.png"]
        );
        assert_eq!(
            file_paths_from_value(&json!("[\"/tmp/c.png\",\"/tmp/d.png\"]")),
            vec!["/tmp/c.png", "/tmp/d.png"]
        );
    }

    #[test]
    fn treats_plain_text_as_single_path() {
        assert_eq!(
            file_paths_from_value(&json!("/tmp/a.png")),
            vec!["/tmp/a.png"]
        );
        assert!(file_paths_from_value(&json!("   ")).is_empty());
        assert!(file_paths_from_value(&json!({ "file": "/tmp/a.png" })).is_empty());
    }
}
