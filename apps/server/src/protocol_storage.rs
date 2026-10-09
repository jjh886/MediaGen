use crate::{protocol_api::error, ServerState};
use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) fn stored(state: &ServerState, key: &str, id: &str) -> Option<Value> {
    let owner = crate::access_control::hash_secret(key);
    state
        .store
        .list_tasks(None)
        .into_iter()
        .filter(|task| task.task_kind == "text")
        .filter_map(|task| serde_json::from_str::<Value>(task.result_data.as_deref()?).ok())
        .find(|value| value["owner"] == owner && value["response"]["id"] == id)
}

pub(crate) fn previous_input(
    state: &ServerState,
    key: &str,
    request: &mut Value,
) -> Result<(), String> {
    let Some(id) = request["previous_response_id"].as_str() else {
        return Ok(());
    };
    let prior = stored(state, key, id).ok_or("previous_response_id 不存在或无权访问")?;
    if prior.get("model_id").unwrap_or(&prior["response"]["model"]) != &request["model"] {
        return Err("previous_response_id 必须使用相同模型".into());
    }
    let mut input = prior["input"].as_array().cloned().unwrap_or_default();
    input.extend(crate::protocol_output::continuation(&prior["response"]));
    input.extend(input_items(&request["input"]));
    request["input"] = json!(input);
    request
        .as_object_mut()
        .ok_or("请求必须是对象")?
        .remove("previous_response_id");
    Ok(())
}

pub(crate) fn input_items(input: &Value) -> Vec<Value> {
    if let Some(text) = input.as_str() {
        vec![json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]})]
    } else {
        input.as_array().cloned().unwrap_or_default()
    }
}

pub(crate) async fn retrieve(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    match stored(&state, &key, &path) {
        Some(value) => HttpResponse::Ok().json(&value["response"]),
        None => error(404, "response_not_found", "响应不存在或未保存"),
    }
}

pub(crate) async fn delete(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let Some(value) = stored(&state, &key, &path) else {
        return error(404, "response_not_found", "响应不存在或未保存");
    };
    if let Some(task_id) = value["task_id"].as_str() {
        state
            .store
            .complete_task(task_id, json!({"status":"deleted"}));
    }
    HttpResponse::Ok().json(json!({"id":path.as_str(),"object":"response.deleted","deleted":true}))
}

#[derive(Deserialize)]
pub(crate) struct Page {
    limit: Option<usize>,
    after: Option<String>,
    order: Option<String>,
}
pub(crate) async fn inputs(
    state: web::Data<ServerState>,
    request: HttpRequest,
    path: web::Path<String>,
    page: web::Query<Page>,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let Some(value) = stored(&state, &key, &path) else {
        return error(404, "response_not_found", "响应不存在或未保存");
    };
    let limit = page.limit.unwrap_or(20);
    if !(1..=100).contains(&limit)
        || page
            .order
            .as_deref()
            .is_some_and(|order| !matches!(order, "asc" | "desc"))
    {
        return error(400, "invalid_pagination", "分页参数无效");
    }
    let mut items = value["input"].as_array().cloned().unwrap_or_default();
    for (index, item) in items.iter_mut().enumerate() {
        if item.get("id").is_none() {
            item["id"] = json!(format!("{}_input_{index}", path.as_str()));
        }
    }
    if page.order.as_deref().unwrap_or("desc") == "desc" {
        items.reverse();
    }
    if let Some(after) = &page.after {
        let Some(index) = items.iter().position(|item| item["id"] == *after) else {
            return error(400, "invalid_cursor", "after 不存在");
        };
        items.drain(..=index);
    }
    let has_more = items.len() > limit;
    items.truncate(limit);
    HttpResponse::Ok().json(json!({"object":"list","first_id":items.first().and_then(|item|item.get("id")),"last_id":items.last().and_then(|item|item.get("id")),"has_more":has_more,"data":items}))
}

/// 请求附带的元数据与插件输出共同保留；生成结果中的同名字段优先。
pub(crate) fn response_metadata(response: &Value, request: &Value) -> Value {
    let mut metadata = request.as_object().cloned().unwrap_or_default();
    if let Some(provider) = response["metadata"].as_object() {
        metadata.extend(provider.clone());
    }
    Value::Object(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};

    #[actix_web::test]
    async fn conversation_titles_survive_storage_http_and_streaming_adapters() {
        let state = crate::protocol_api_tests::state();
        let script = state
            .plugin_loader
            .get_plugin_dir("test")
            .unwrap()
            .join("scripts/text.js");
        let source = std::fs::read_to_string(&script).unwrap().replace(
            "status:'completed',model:",
            "status:'completed',metadata:{conversation_id:'upstream-id',conversation_title:'杯子广告'},model:",
        );
        std::fs::write(script, source).unwrap();
        let app = test::init_service(
            App::new()
                .app_data(state)
                .configure(crate::configure_routes),
        )
        .await;
        for endpoint in ["responses", "chat/completions"] {
            for streaming in [false, true] {
                let mut body = json!({"model":"text-model","stream":streaming,"metadata":{"client":"retained","conversation_title":"caller-title"}});
                if endpoint == "responses" {
                    body["input"] = json!("你好");
                } else {
                    body["messages"] = json!([{"role":"user","content":"你好"}]);
                }
                let result = test::call_service(
                    &app,
                    test::TestRequest::post()
                        .uri(&format!("/v1/{endpoint}"))
                        .insert_header(("authorization", "Bearer key-a"))
                        .set_json(body)
                        .to_request(),
                )
                .await;
                assert_eq!(result.status(), 200);
                let bytes = test::read_body(result).await;
                let response: Value = if streaming {
                    String::from_utf8(bytes.to_vec())
                        .unwrap()
                        .lines()
                        .filter_map(|line| line.strip_prefix("data: "))
                        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
                        .find_map(|event| {
                            if endpoint == "responses" && event["type"] == "response.completed" {
                                Some(event["response"].clone())
                            } else if endpoint != "responses"
                                && event["choices"][0]["finish_reason"] == "stop"
                            {
                                Some(event)
                            } else {
                                None
                            }
                        })
                        .expect("缺少完成事件")
                } else {
                    serde_json::from_slice(&bytes).unwrap()
                };
                assert_eq!(response["metadata"]["conversation_title"], "杯子广告");
                assert_eq!(response["metadata"]["conversation_id"], "upstream-id");
                assert_eq!(response["metadata"]["client"], "retained");
                if endpoint == "responses" {
                    let stored: Value = test::call_and_read_body_json(
                        &app,
                        test::TestRequest::get()
                            .uri(&format!(
                                "/v1/responses/{}",
                                response["id"].as_str().unwrap()
                            ))
                            .insert_header(("authorization", "Bearer key-a"))
                            .to_request(),
                    )
                    .await;
                    assert_eq!(stored["metadata"], response["metadata"]);
                }
            }
        }
    }
}
