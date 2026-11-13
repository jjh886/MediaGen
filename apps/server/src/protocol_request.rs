use serde_json::{json, Value};

pub(crate) fn validate_request(body: &Value, endpoint: &str) -> Result<(), String> {
    if !body.is_object() {
        return Err("请求体必须是 JSON 对象".into());
    }
    let fields: &[&str] = match endpoint {
        "speech" => &[
            "model",
            "input",
            "voice",
            "response_format",
            "speed",
            "instructions",
            "stream_format",
            "stream",
        ],
        "chat" => &[
            "async",
            "callback_url",
            "model",
            "messages",
            "audio",
            "frequency_penalty",
            "function_call",
            "functions",
            "logit_bias",
            "logprobs",
            "max_completion_tokens",
            "max_tokens",
            "metadata",
            "modalities",
            "n",
            "parallel_tool_calls",
            "prediction",
            "presence_penalty",
            "prompt_cache_key",
            "prompt_cache_retention",
            "reasoning_effort",
            "response_format",
            "safety_identifier",
            "seed",
            "service_tier",
            "stop",
            "store",
            "stream",
            "stream_options",
            "temperature",
            "tool_choice",
            "tools",
            "top_logprobs",
            "top_p",
            "user",
            "verbosity",
            "web_search_options",
        ],
        "compact" => &[
            "model",
            "input",
            "instructions",
            "previous_response_id",
            "reasoning",
        ],
        _ => &[
            "async",
            "callback_url",
            "model",
            "input",
            "instructions",
            "background",
            "context_management",
            "conversation",
            "include",
            "max_output_tokens",
            "max_tool_calls",
            "metadata",
            "parallel_tool_calls",
            "previous_response_id",
            "prompt",
            "prompt_cache_key",
            "prompt_cache_retention",
            "reasoning",
            "safety_identifier",
            "service_tier",
            "store",
            "stream",
            "stream_options",
            "temperature",
            "text",
            "tool_choice",
            "tools",
            "top_logprobs",
            "top_p",
            "truncation",
            "user",
            "options",
        ],
    };
    if let Some(key) = body
        .as_object()
        .unwrap()
        .keys()
        .find(|key| !fields.contains(&key.as_str()))
    {
        return Err(format!("不支持参数 {key}"));
    }
    if body["model"]
        .as_str()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err("model 必须是非空字符串".into());
    }
    for field in ["stream", "store", "background", "parallel_tool_calls"] {
        if body
            .get(field)
            .is_some_and(|value| !value.is_null() && !value.is_boolean())
        {
            return Err(format!("{field} 必须是布尔值"));
        }
    }
    if body.get("async").is_some_and(|value| !value.is_boolean()) {
        return Err("async 必须是布尔值".into());
    }
    if body["async"] == true && body["stream"] == true {
        return Err("async 排队模式通过任务接口查询结果，不能同时启用 stream".into());
    }
    if let Some(url) = body.get("callback_url") {
        if body["async"] != true {
            return Err("callback_url 仅在 async=true 时生效".into());
        }
        crate::generation_queue::callback::validate_url(
            url.as_str().ok_or("callback_url 必须是字符串")?.trim(),
        )?;
    }
    if body["background"] == true {
        return Err("当前插件不支持 background".into());
    }
    for field in ["max_output_tokens", "max_completion_tokens", "max_tokens"] {
        if body
            .get(field)
            .is_some_and(|value| value.as_u64().is_none_or(|n| n == 0))
        {
            return Err(format!("{field} 必须是正整数"));
        }
    }
    if body.get("tools").is_some_and(|value| !value.is_array()) {
        return Err("tools 必须是数组".into());
    }
    if body.get("metadata").is_some_and(|value| {
        value.as_object().is_none_or(|object| {
            object.len() > 16
                || object.iter().any(|(key, value)| {
                    key.chars().count() > 64
                        || value.as_str().is_none_or(|text| text.chars().count() > 512)
                })
        })
    }) {
        return Err("metadata 必须为最多 16 项的字符串映射".into());
    }
    if endpoint == "speech"
        && body
            .get("stream_format")
            .is_some_and(|value| value != "audio")
    {
        return Err("当前语音模型仅支持 stream_format=audio".into());
    }
    match endpoint {
        "speech" => {
            let input = body["input"].as_str().ok_or("input 必须是字符串")?;
            if input.trim().is_empty() || input.chars().count() > 100000 {
                return Err("input 长度必须为 1–100000 个字符".into());
            }
            if body["voice"]
                .as_str()
                .is_none_or(|voice| voice.trim().is_empty())
            {
                return Err("voice 必须是非空字符串".into());
            }
        }
        "chat" => {
            if body["store"] == true {
                return Err(
                    "Chat Completions 暂不支持 store=true，请使用 Responses 保存和查询会话".into(),
                );
            }
            let messages = body["messages"].as_array().ok_or("messages 必须是数组")?;
            if messages.is_empty() {
                return Err("messages 不能为空".into());
            }
            for message in messages {
                if !matches!(
                    message["role"].as_str(),
                    Some("system" | "developer" | "user" | "assistant" | "tool")
                ) {
                    return Err("messages.role 无效".into());
                }
                if message["role"] == "tool" && message["tool_call_id"].as_str().is_none() {
                    return Err("工具结果缺少 tool_call_id".into());
                }
            }
            if body.get("n").is_some_and(|value| value.as_u64() != Some(1)) {
                return Err("当前插件仅支持 n=1".into());
            }
        }
        "responses" | "compact" => {
            if endpoint == "compact" && body["stream"] == true {
                return Err("压缩接口不支持 stream".into());
            }
            if !body["input"].is_string() && !body["input"].is_array() {
                return Err("input 必须是字符串或数组".into());
            }
        }
        _ => {}
    }
    reject_local_files(body)?;
    Ok(())
}

fn reject_local_files(value: &Value) -> Result<(), String> {
    match value {
        Value::Object(object) => {
            if matches!(
                value["type"].as_str(),
                Some("input_file" | "input_image" | "file")
            ) {
                let file = value.get("file").unwrap_or(value);
                if file.get("file_path").is_some() || file.get("path").is_some() {
                    return Err(
                        "公共接口不接受本地文件路径，请先上传 /v1/files 并使用 file_id".into(),
                    );
                }
            }
            for value in object.values() {
                reject_local_files(value)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                reject_local_files(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn chat_to_responses(body: &Value) -> Result<Value, String> {
    let mut request = body.clone();
    let mut input = Vec::new();
    for message in body["messages"].as_array().ok_or("messages 必须是数组")? {
        let role = message["role"].as_str().ok_or("消息缺少 role")?;
        if role == "tool" {
            input.push(json!({"type":"function_call_output","call_id":message["tool_call_id"],"output":message["content"]}));
            continue;
        }
        if !message["content"].is_null() {
            let content = match &message["content"] {
                Value::String(text) => json!([{ "type": if role == "assistant" {"output_text"} else {"input_text"}, "text":text }]),
                Value::Array(parts) => Value::Array(parts.iter().map(|part| {
                    match part["type"].as_str() {
                        Some("text") => Ok(json!({"type":if role == "assistant" {"output_text"} else {"input_text"},"text":part["text"]})),
                        Some("image_url") => Ok(json!({"type":"input_image","image_url":part["image_url"]["url"],"detail":part["image_url"].get("detail").cloned().unwrap_or(json!("auto"))})),
                        Some("input_audio") => Ok(json!({"type":"input_audio","input_audio":part["input_audio"]})),
                        Some("file") => {
                            if !part["file"].is_object() { return Err("file 必须是对象".into()); }
                            let mut file = part["file"].clone();
                            file["type"] = json!("input_file");
                            Ok(file)
                        }
                        _ => Err("当前文本通道不支持此消息内容类型".to_string()),
                    }
                }).collect::<Result<Vec<_>,_>>()?),
                _ => return Err("messages.content 必须是字符串或数组".into()),
            };
            input.push(json!({"type":"message","role":role,"content":content}));
        }
        if role == "assistant" && message["media"].is_array() {
            input.extend(crate::protocol_output::continuation(
                &json!({"output":message["media"]}),
            ));
        }
        if let Some(calls) = message["tool_calls"].as_array() {
            for call in calls {
                if call["type"] != "function" {
                    return Err("不支持此 tool_call 类型".into());
                }
                input.push(json!({"type":"function_call","call_id":call["id"],"name":call["function"]["name"],"arguments":call["function"]["arguments"]}));
            }
        }
    }
    request["input"] = json!(input);
    if let Some(effort) = body.get("reasoning_effort") {
        request["reasoning"] = json!({"effort":effort});
    }
    if let Some(format) = body.get("response_format") {
        request["text"] = if format["type"] == "json_schema" {
            let mut schema = format["json_schema"].clone();
            schema["type"] = json!("json_schema");
            json!({"format":schema})
        } else {
            json!({"format":format})
        };
    }
    if let Some(tokens) = body
        .get("max_completion_tokens")
        .or_else(|| body.get("max_tokens"))
    {
        request["max_output_tokens"] = tokens.clone();
    }
    if let Some(map) = request.as_object_mut() {
        for field in [
            "messages",
            "n",
            "stream_options",
            "reasoning_effort",
            "response_format",
            "max_tokens",
            "max_completion_tokens",
        ] {
            map.remove(field);
        }
    }
    Ok(request)
}

pub(crate) fn chat_response(response: &Value, model: &str) -> Value {
    let model = response["model"]
        .as_str()
        .filter(|model| !model.is_empty())
        .unwrap_or(model);
    let mut text = String::new();
    let mut refusal = String::new();
    let mut calls = Vec::new();
    for item in response["output"].as_array().into_iter().flatten() {
        if item["type"] == "function_call" {
            calls.push(json!({"id":item["call_id"],"type":"function","function":{"name":item["name"],"arguments":item["arguments"]}}));
        }
        for part in item["content"].as_array().into_iter().flatten() {
            if let Some(value) = part["text"].as_str() {
                text.push_str(value);
            }
            if let Some(value) = part["refusal"].as_str() {
                refusal.push_str(value);
            }
        }
    }
    let mut message = json!({"role":"assistant","content":if text.is_empty() {Value::Null} else {json!(text)},"refusal":if refusal.is_empty() {Value::Null} else {json!(refusal)}});
    crate::protocol_output::enrich_chat(&mut message, response);
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    json!({"id":response["id"].as_str().unwrap_or("").replace("resp_", "chatcmpl-"),"object":"chat.completion","created":response["created_at"],"model":model,"choices":[{"index":0,"message":message,"finish_reason":finish_reason(response, !calls.is_empty()),"logprobs":null}],"usage":chat_usage(&response["usage"]),"metadata":response["metadata"].as_object().cloned().unwrap_or_default()})
}

pub(crate) fn finish_reason(response: &Value, tools: bool) -> &'static str {
    if response["status"] == "incomplete" {
        if response["incomplete_details"]["reason"] == "content_filter" {
            "content_filter"
        } else {
            "length"
        }
    } else if tools {
        "tool_calls"
    } else {
        "stop"
    }
}

pub(crate) fn chat_usage(usage: &Value) -> Value {
    if usage.is_null() {
        return Value::Null;
    }
    json!({"prompt_tokens":usage["input_tokens"],"completion_tokens":usage["output_tokens"],"total_tokens":usage["total_tokens"],"prompt_tokens_details":usage["input_tokens_details"],"completion_tokens_details":usage["output_tokens_details"]})
}

pub(crate) fn validate_capabilities(body: &Value, capabilities: &Value) -> Result<(), String> {
    if let Some(speed) = body.get("speed").filter(|v| !v.is_null()) {
        let speed = speed.as_f64().ok_or("speed 必须是数字")?;
        if capabilities["speed"]["minimum"]
            .as_f64()
            .is_some_and(|min| speed < min)
            || capabilities["speed"]["maximum"]
                .as_f64()
                .is_some_and(|max| speed > max)
        {
            return Err("当前模型不支持此 speed".into());
        }
    }
    if let Some(effort) = body["reasoning"]["effort"].as_str() {
        if capabilities["reasoning"]["efforts"]
            .as_array()
            .is_some_and(|efforts| !efforts.contains(&json!(effort)))
        {
            return Err("当前模型不支持此思考深度".into());
        }
    }
    if capabilities["tool_calling"] == false
        && body["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
    {
        return Err("当前模型不支持原生工具调用".into());
    }
    for field in capabilities["unsupported_parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if body.get(field).is_some_and(|value| !value.is_null()) {
            return Err(format!("当前模型不支持参数 {field}"));
        }
    }
    for (field, choices) in [("voice", "voices"), ("response_format", "response_formats")] {
        if let (Some(value), Some(allowed)) = (body.get(field), capabilities[choices].as_array()) {
            if !allowed.contains(value) {
                return Err(format!("当前模型不支持此 {field}"));
            }
        }
    }
    if body["stream"] == true && capabilities["streaming"] == false {
        return Err("当前模型不支持流式输出".into());
    }
    if let Some(input) = body["input"].as_array() {
        for item in input {
            for part in item["content"].as_array().into_iter().flatten() {
                let modality = match part["type"].as_str() {
                    Some("input_image") => "image",
                    Some("input_file") => "file",
                    Some("input_audio") => "audio",
                    _ => "text",
                };
                if modality == "image"
                    && capabilities["image_sources"]
                        .as_array()
                        .is_some_and(|sources| !sources.contains(&json!("url")))
                    && part["image_url"]
                        .as_str()
                        .is_some_and(|url| !url.starts_with("data:"))
                {
                    return Err("当前模型的图片输入需使用 data URL 或 file_id".into());
                }
                if capabilities["input_modalities"]
                    .as_array()
                    .is_some_and(|values| !values.contains(&json!(modality)))
                {
                    return Err(format!("当前模型不支持 {modality} 输入"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "protocol_request_tests.rs"]
mod tests;
