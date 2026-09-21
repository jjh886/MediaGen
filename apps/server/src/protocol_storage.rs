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
    input.extend(
        prior["response"]["output"]
            .as_array()
            .into_iter()
            .flatten()
            .cloned(),
    );
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
