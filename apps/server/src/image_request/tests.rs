use serde_json::{json, Value};

use super::*;

#[test]
fn applies_defaults_for_generation_and_variation() {
    let mut generation = OpenAiImageRequest::empty();
    generation.prompt = Some(Value::String("draw".to_string()));
    generation.apply_endpoint_defaults(ImageEndpoint::Generation);
    assert_eq!(generation.model, "gpt-image-1");
    assert_eq!(generation.prompt_text(), "draw");

    let mut variation = OpenAiImageRequest::empty();
    variation.images.push(OpenAiImageRef {
        image_url: Some("https://example.test/a.png".to_string()),
        file_id: None,
    });
    variation.apply_endpoint_defaults(ImageEndpoint::Variation);
    assert_eq!(variation.model, "gpt-image-1");
    assert_eq!(variation.prompt_text(), "Create a variation of the image.");
}

#[test]
fn validates_required_fields_and_unsupported_inputs() {
    let empty = OpenAiImageRequest::empty();
    assert!(empty.validate(ImageEndpoint::Generation).is_err());

    let mut request = OpenAiImageRequest::empty();
    request.model = "gpt-image-1".to_string();
    request.prompt = Some(Value::String("draw".to_string()));
    request.response_format = Some("url".to_string());
    assert_eq!(
        request.validate(ImageEndpoint::Generation).unwrap_err().1,
        "unsupported_parameter"
    );

    request.response_format = Some("b64_json".to_string());
    request.output_format = Some("gif".to_string());
    assert!(request
        .validate(ImageEndpoint::Generation)
        .unwrap_err()
        .2
        .contains("output_format"));
}

#[test]
fn validates_edit_reference_and_numeric_limits() {
    let mut request = OpenAiImageRequest::empty();
    request.model = "gpt-image-1".to_string();
    request.prompt = Some(Value::String("edit".to_string()));
    assert!(request.validate(ImageEndpoint::Edit).is_err());

    request.images.push(OpenAiImageRef {
        image_url: Some("https://example.test/ref.png".to_string()),
        file_id: None,
    });
    request.partial_images = Some(4);
    assert!(request
        .validate(ImageEndpoint::Edit)
        .unwrap_err()
        .2
        .contains("partial_images"));

    request.partial_images = Some(3);
    request.output_compression = Some(101);
    assert!(request
        .validate(ImageEndpoint::Edit)
        .unwrap_err()
        .2
        .contains("output_compression"));
}

#[test]
fn converts_prompt_arrays_and_flow_request() {
    let request = OpenAiImageRequest {
        model: "remote-model".to_string(),
        prompt: Some(json!([{ "text": "line one" }, "line two"])),
        n: Some(9),
        size: Some("1536x1024".to_string()),
        quality: Some("high".to_string()),
        reasoning_effort: Some("extended".to_string()),
        output_format: Some("webp".to_string()),
        background: Some("transparent".to_string()),
        images: vec![OpenAiImageRef {
            image_url: Some(" https://example.test/ref.png ".trim().to_string()),
            file_id: None,
        }],
        mask: Some(OpenAiImageRef {
            image_url: Some("https://example.test/mask.png".to_string()),
            file_id: None,
        }),
        ..OpenAiImageRequest::empty()
    };

    let flow = request.to_flow_request(
        "Remote Model".to_string(),
        ImageEndpoint::Edit,
        vec!["data:image/png;base64,abc".to_string()],
        Some("data:image/png;base64,mask".to_string()),
    );

    assert_eq!(flow.prompt, "line one\nline two");
    assert_eq!(flow.max_images, 4);
    assert_eq!(flow.output_format, "webp");
    assert_eq!(flow.quality, "high");
    assert_eq!(flow.reasoning_effort.as_deref(), Some("extended"));
    assert_eq!(flow.background, "transparent");
    assert_eq!(flow.image_aspect_ratio, "3:2");
    assert_eq!(flow.action, "edit");
    assert_eq!(flow.ref_image_urls, vec!["https://example.test/ref.png"]);
    assert_eq!(
        flow.mask_image_url.as_deref(),
        Some("https://example.test/mask.png")
    );
    assert_eq!(
        flow.mask_image_dataurl.as_deref(),
        Some("data:image/png;base64,mask")
    );
}

#[test]
fn keeps_file_id_until_api_layer_resolves_it() {
    let mut request = OpenAiImageRequest::empty();
    request.model = "gpt-image-1".to_string();
    request.prompt = Some(Value::String("draw".to_string()));
    request.images.push(OpenAiImageRef {
        file_id: Some("file-123".to_string()),
        image_url: None,
    });
    assert!(request.validate(ImageEndpoint::Generation).is_ok());

    request.images.clear();
    request.size = Some("1024x1536".to_string());
    assert_eq!(request.image_aspect_ratio(), "2:3");
    request.image_aspect_ratio = Some(" 4:3 ".to_string());
    assert_eq!(request.image_aspect_ratio(), "4:3");
}

#[test]
fn validates_async_callback_url_extension() {
    let mut request = OpenAiImageRequest::empty();
    request.model = "gpt-image-1".to_string();
    request.prompt = Some(Value::String("draw".to_string()));
    request.callback_url = Some("https://example.test/callback".to_string());
    assert_eq!(
        request.validate(ImageEndpoint::Generation).unwrap_err().1,
        "unsupported_parameter"
    );

    request.async_generation = true;
    request.callback_url = Some("file:///tmp/result.json".to_string());
    assert!(request
        .validate(ImageEndpoint::Generation)
        .unwrap_err()
        .2
        .contains("http"));

    let aliased: OpenAiImageRequest = serde_json::from_value(json!({
        "model": "gpt-image-1",
        "prompt": "draw",
        "async": true,
        "webhook_url": " https://example.test/hook "
    }))
    .unwrap();
    assert_eq!(
        aliased.callback_url().as_deref(),
        Some("https://example.test/hook")
    );
    assert!(aliased.validate(ImageEndpoint::Generation).is_ok());
}

#[test]
fn omitted_reasoning_keeps_existing_recovery_identity_shape() {
    let original = serde_json::to_value(OpenAiImageRequest::empty()).unwrap();
    assert!(original.get("reasoning_effort").is_none());
    let mut body = original.clone();
    body["reasoning_effort"] = json!("extended");
    let parsed: OpenAiImageRequest = serde_json::from_value(body).unwrap();
    assert_eq!(
        serde_json::to_value(parsed).unwrap()["reasoning_effort"],
        "extended"
    );
}
