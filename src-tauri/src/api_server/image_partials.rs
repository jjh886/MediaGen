use super::ImageEndpoint;

pub(crate) struct PartialImageEvent {
    pub index: u32,
    pub b64_json: String,
    pub output_format: String,
}

pub(crate) fn format_partial_sse_event(
    endpoint: ImageEndpoint,
    partial: &PartialImageEvent,
) -> String {
    let event = match endpoint {
        ImageEndpoint::Generation => "image_generation.partial_image",
        ImageEndpoint::Edit => "image_edit.partial_image",
    };
    let payload = serde_json::json!({
        "type": event,
        "partial_image_index": partial.index,
        "b64_json": partial.b64_json,
        "output_format": partial.output_format,
    });
    format!("event: {event}\ndata: {payload}\n\n")
}

pub(crate) fn collect_partial_images(
    poll_data: Option<&serde_json::Value>,
    output_format: &str,
) -> Vec<PartialImageEvent> {
    let Some(items) = poll_data.and_then(|data| data["partial_images"].as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(fallback_index, item)| {
            partial_image_event(item, fallback_index, output_format)
        })
        .collect()
}

fn partial_image_event(
    item: &serde_json::Value,
    fallback_index: usize,
    output_format: &str,
) -> Option<PartialImageEvent> {
    let b64_json = item["b64_json"]
        .as_str()
        .or_else(|| item["partial_image_b64"].as_str())?
        .to_string();
    let index = item["partial_image_index"]
        .as_u64()
        .unwrap_or(fallback_index as u64) as u32;
    Some(PartialImageEvent {
        index,
        b64_json,
        output_format: item["output_format"]
            .as_str()
            .unwrap_or(output_format)
            .to_string(),
    })
}
