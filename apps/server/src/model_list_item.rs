use serde_json::{Map, Value};

pub(super) fn summary(model: &Value) -> Value {
    let mut result = fields(model, &["id", "object", "created", "owned_by"]);
    let caps = &model["capabilities"];
    let mut selected = fields(
        caps,
        &[
            "context_window",
            "max_context_window",
            "max_input_tokens",
            "max_output_tokens",
            "input_modalities",
            "file_extensions",
            "format_examples",
            "supported_mime_types",
        ],
    );
    for (key, names) in [
        (
            "pricing",
            &[
                "currency",
                "unit",
                "input",
                "cached_input",
                "output",
                "rates",
                "long_context",
                "valid_through",
                "next_rates",
                "note",
                "reference_only",
                "status",
            ][..],
        ),
        ("reasoning", &["efforts", "default_effort", "parameter"][..]),
        (
            "attachment_limits",
            &[
                "file_bytes",
                "image_bytes",
                "audio_bytes",
                "video_bytes",
                "request_body_bytes",
                "max_files",
                "max_reference_images",
                "max_reference_videos",
                "max_reference_video_seconds",
                "web_defaults",
                "spreadsheet_bytes_approx",
                "document_tokens",
                "note",
            ][..],
        ),
        (
            "limits",
            &["max_reference_images", "max_reference_videos"][..],
        ),
    ] {
        let values = fields(&caps[key], names);
        if !values.is_empty() {
            selected.insert(key.into(), Value::Object(values));
        }
    }
    if !selected.is_empty() {
        result.insert("capabilities".into(), Value::Object(selected));
    }
    Value::Object(result)
}

fn fields(value: &Value, names: &[&str]) -> Map<String, Value> {
    names
        .iter()
        .filter_map(|&name| {
            value
                .get(name)
                .map(|field| (name.to_owned(), field.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_media_attachment_counts_and_omits_empty_extensions() {
        let mut model = json!({"id":"image-model","object":"model","created":0,"owned_by":"test"});
        let standard = model.clone();
        model["capabilities"] = json!({"limits":{"supports_edit":true},"streaming":true});
        assert_eq!(summary(&model), standard);
        model["capabilities"]["limits"]["max_reference_images"] = json!(0);
        model["capabilities"]["limits"]["max_reference_videos"] = json!(1);
        assert_eq!(
            summary(&model)["capabilities"],
            json!({"limits":{"max_reference_images":0,"max_reference_videos":1}})
        );
    }
}
