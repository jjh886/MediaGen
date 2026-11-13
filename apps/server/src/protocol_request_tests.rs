use super::*;
#[test]
fn chat_attachments_preserve_audio_bytes_and_reject_non_object_files() {
    let body = json!({"model":"test","messages":[{"role":"user","content":[
        {"type":"input_audio","input_audio":{"format":"mp3","data":"YWJj"}},
        {"type":"file","file":{"file_id":"file-owned"}}
    ]}]});
    let converted = chat_to_responses(&body).unwrap();
    assert_eq!(
        converted["input"][0]["content"][0],
        json!({"type":"input_audio","input_audio":{"format":"mp3","data":"YWJj"}})
    );
    assert_eq!(converted["input"][0]["content"][1]["file_id"], "file-owned");
    assert!(chat_to_responses(
        &json!({"messages":[{"role":"user","content":[{"type":"file","file":"invalid"}]}]})
    )
    .is_err());
}

#[test]
fn rejects_invalid_scalar_types_and_metadata_without_silent_coercion() {
    for (field, value) in [
        ("unknown", json!(true)),
        ("stream", json!("true")),
        ("store", json!(1)),
        ("background", json!(true)),
        ("max_output_tokens", json!(-1)),
        ("max_tokens", json!(0)),
        ("tools", json!({})),
        ("metadata", json!({"label":3})),
        ("metadata", json!({"label":"x".repeat(513)})),
    ] {
        let mut request = json!({"model":"test","input":"hello"});
        request[field] = value;
        assert!(validate_request(&request, "responses").is_err(), "{field}");
    }
    assert!(validate_request(
        &json!({"model":"test","input":"hello","max_output_tokens":32,"metadata":{"用途":"测试"}}),
        "responses"
    )
    .is_ok());
    assert!(validate_request(
        &json!({"model":"test","messages":[{"role":"user","content":"hello"}],"n":2}),
        "chat"
    )
    .is_err());
    assert!(validate_request(
        &json!({"model":"test","input":"hello","voice":"voice","stream_format":"sse"}),
        "speech"
    )
    .is_err());
}
#[test]
fn capability_validation_checks_actual_account_modalities_efforts_and_formats() {
    let capabilities = json!({"input_modalities":["text","image"],"image_sources":["data_url","file_id"],"reasoning":{"efforts":["low","high"]},"tool_calling":false,"streaming":false,"voices":["voice"],"response_formats":["wav"],"unsupported_parameters":["temperature"],"speed":{"minimum":1,"maximum":1}});
    for request in [
        json!({"speed":2}),
        json!({"speed":"1"}),
        json!({"reasoning":{"effort":"ultra"}}),
        json!({"tools":[{"type":"function","name":"f"}]}),
        json!({"stream":true}),
        json!({"voice":"missing"}),
        json!({"response_format":"mp3"}),
        json!({"temperature":0.5}),
        json!({"input":[{"content":[{"type":"input_image","image_url":"https://example.test/image.png"}]}]}),
        json!({"input":[{"content":[{"type":"input_audio"}]}]}),
    ] {
        assert!(validate_capabilities(&request, &capabilities).is_err());
    }
    assert!(validate_capabilities(&json!({"voice":"voice","response_format":"wav","reasoning":{"effort":"high"},"input":[{"content":[{"type":"input_image","image_url":"data:image/png;base64,AAAA"}]}]}),&capabilities).is_ok());
}
