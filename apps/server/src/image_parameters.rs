use serde_json::{json, Value};

use crate::image_request::{ImageEndpoint, OpenAiImageRequest};

pub(crate) fn model_limits(limits: &Value, capabilities: &Value) -> Value {
    let mut limits = limits.as_object().cloned().unwrap_or_default();
    if let Some(values) = capabilities["parameter_values"].as_object() {
        let mut parameters = limits
            .get("parameter_values")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        parameters.extend(values.clone());
        limits.insert("parameter_values".into(), Value::Object(parameters));
    }
    Value::Object(limits)
}

pub(crate) fn schema(limits: &Value) -> Value {
    let mut properties = json!({
        "model":{"type":"string"}, "prompt":{"type":"string"},
        "n":{"type":"integer","minimum":1,"maximum":4}, "size":{"type":"string","default":"auto"},
        "quality":{"type":"string","enum":["auto","low","medium","high","xhigh","max"]},
        "background":{"type":"string","enum":["auto","opaque","transparent"]},
        "response_format":{"type":"string","enum":["b64_json"]},
        "output_format":{"type":"string","enum":["png","jpeg","webp"]},
        "output_compression":{"type":"integer","minimum":0,"maximum":100},
        "stream":{"type":"boolean"}, "partial_images":{"type":"integer","minimum":0,"maximum":3},
        "moderation":{"type":"string","enum":["auto","low"]},
        "input_fidelity":{"type":"string","enum":["low","high"]}, "user":{"type":"string"},
        "images":{"type":"array","items":{"type":"object","properties":{"image_url":{"type":"string"},"file_id":{"type":"string"}}}},
        "mask":{"type":"object","properties":{"image_url":{"type":"string"},"file_id":{"type":"string"}}}
    });
    for (key, values) in limits["parameter_values"].as_object().into_iter().flatten() {
        if values.as_array().is_some_and(Vec::is_empty) {
            properties.as_object_mut().unwrap().remove(key);
        } else if properties.get(key).is_some() {
            properties[key]["enum"] = values.clone();
        }
    }
    if limits["supports_mask"] == false {
        properties.as_object_mut().unwrap().remove("mask");
    }
    json!({"type":"object","required":["model","prompt"],"properties":properties})
}

/// 插件声明取值和能力，协议边界统一执行；不能静默忽略客户端参数。
pub(crate) fn validate(
    body: &OpenAiImageRequest,
    endpoint: ImageEndpoint,
    limits: &Value,
) -> Result<(), String> {
    let flag = match endpoint {
        ImageEndpoint::Generation => "supports_generation",
        ImageEndpoint::Edit => "supports_edit",
        ImageEndpoint::Variation => "supports_variation",
    };
    if limits[flag] == false {
        return Err("当前模型不支持此图片接口".into());
    }
    if body.mask.is_some() && limits["supports_mask"] == false {
        return Err("当前模型不支持 mask".into());
    }
    if limits["max_reference_images"]
        .as_u64()
        .is_some_and(|n| body.images.len() as u64 > n)
    {
        return Err("参考图片数量超出当前模型限制".into());
    }
    let values = serde_json::to_value(body).map_err(|_| "图片参数无效")?;
    for (field, allowed) in limits["parameter_values"].as_object().into_iter().flatten() {
        if let (Some(value), Some(allowed)) = (
            values.get(field).filter(|v| !v.is_null()),
            allowed.as_array(),
        ) {
            if !allowed.contains(value) {
                return Err(format!("当前模型不支持此 {field}，请查询模型能力"));
            }
        }
    }
    if body.output_compression.is_some() {
        if let Some(formats) = limits["compression_formats"].as_array() {
            if !formats.iter().any(|v| v == &values["output_format"]) {
                return Err("当前模型不支持此输出格式的 output_compression".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enforces_plugin_parameters_and_does_not_invent_platform_defaults() {
        let mut body = OpenAiImageRequest::empty();
        let limits = json!({"supports_mask":false,"parameter_values":{"quality":["auto"]}});
        assert!(validate(&body, ImageEndpoint::Generation, &limits).is_ok());
        body.quality = Some("high".into());
        assert!(validate(&body, ImageEndpoint::Generation, &limits)
            .unwrap_err()
            .contains("quality"));
        assert!(validate(&body, ImageEndpoint::Generation, &Value::Null).is_ok());
    }
}
