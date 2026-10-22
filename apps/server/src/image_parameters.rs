use serde_json::{json, Value};

use crate::image_request::{ImageEndpoint, OpenAiImageRequest};

pub(crate) fn model_limits(limits: &Value, capabilities: &Value) -> Value {
    let mut limits = limits.as_object().cloned().unwrap_or_default();
    let mut parameters = limits
        .get("parameter_values")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(values) = capabilities["parameter_values"].as_object() {
        parameters.extend(values.clone());
    }
    if let Some(efforts) = capabilities["reasoning"]["efforts"].as_array() {
        parameters.insert("reasoning_effort".into(), json!(efforts));
    }
    // 未声明支持时明确拒绝，防止插件忽略后按默认档位生成。
    parameters.entry("reasoning_effort").or_insert(json!([]));
    limits.insert("parameter_values".into(), Value::Object(parameters));
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
        "reasoning_effort":{"type":"string","minLength":1},
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

    #[test]
    fn reasoning_effort_uses_declared_values_and_never_downgrades() {
        for capabilities in [
            json!({}),
            json!({"reasoning":{"efforts":[]}}),
            json!({"reasoning":{"efforts":["standard","extended"]}}),
        ] {
            let limits = model_limits(&json!({}), &capabilities);
            let supports = capabilities["reasoning"]["efforts"]
                .as_array()
                .is_some_and(|values| !values.is_empty());
            let schema = schema(&limits);
            assert_eq!(
                schema["properties"]["reasoning_effort"].is_object(),
                supports
            );
            for effort in [
                None,
                Some("standard"),
                Some("extended"),
                Some("high"),
                Some("none"),
                Some(""),
            ] {
                let mut body = OpenAiImageRequest::empty();
                body.reasoning_effort = effort.map(str::to_string);
                let expected = effort.is_none()
                    || (supports && matches!(effort, Some("standard" | "extended")));
                assert_eq!(
                    validate(&body, ImageEndpoint::Generation, &limits).is_ok(),
                    expected
                );
                assert_eq!(body.reasoning_effort.as_deref(), effort);
            }
        }
    }

    #[actix_web::test]
    async fn http_image_reasoning_reaches_plugin_and_survives_queue_storage() {
        use actix_web::{test, web, App};
        let mut state = crate::image_api_tests::queue_state("image-reasoning");
        let directory = state.plugin_loader.get_plugin_dir("fake").unwrap();
        let path = directory.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        manifest["models"][0]["capabilities"] =
            json!({"reasoning":{"efforts":["standard","extended"],"default_effort":"standard"}});
        let script = manifest["flows"]["generate_image"]["steps"][0]["script"]
            .as_str()
            .unwrap();
        manifest["flows"]["generate_image"]["steps"][0]["script"] = json!(script.replacen("function main(ctx) {", "function main(ctx) { if(ctx.reasoning_effort!=='extended')throw new Error('lost effort');",1));
        std::fs::write(&path, manifest.to_string()).unwrap();
        state.plugin_loader = std::sync::Arc::new(
            mediagen_plugin_runtime::PluginLoader::load_from(directory.parent().unwrap()).unwrap(),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(state.clone()))
                .configure(crate::configure_routes),
        )
        .await;
        let body = json!({"model":"fake-image","prompt":"draw","reasoning_effort":"extended"});
        for effort in [json!("high"), json!(""), json!(42)] {
            let mut invalid = body.clone();
            invalid["reasoning_effort"] = effort;
            let response = test::call_service(
                &app,
                crate::image_api_tests::image_request()
                    .set_json(invalid)
                    .to_request(),
            )
            .await;
            assert_eq!(response.status(), 400);
            assert!(state.store.list_tasks(None).is_empty());
        }
        let response = test::call_service(
            &app,
            crate::image_api_tests::image_request()
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), 200);
        let mut queued = body.clone();
        queued["async"] = json!(true);
        let response = test::call_service(
            &app,
            crate::image_api_tests::image_request()
                .set_json(queued)
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), 202);
        let response: Value = test::read_body_json(response).await;
        let job = state
            .store
            .generation_job(response["data"]["taskId"].as_str().unwrap(), None)
            .unwrap();
        let restored: OpenAiImageRequest = serde_json::from_str(&job.request_json).unwrap();
        assert_eq!(restored.reasoning_effort.as_deref(), Some("extended"));
        let (manifest, model) = state.plugin_loader.resolve_model("fake-image").unwrap();
        let mut changed = model.clone();
        changed.extra.insert("capabilities".into(), json!({}));
        let error = crate::image_flow_execution::prepare_flow_request(
            &restored,
            &changed,
            manifest,
            ImageEndpoint::Generation,
            "",
        )
        .await
        .unwrap_err();
        assert!(error.contains("reasoning_effort"));
    }
}
