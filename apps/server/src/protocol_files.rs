use crate::ServerState;
use serde_json::{json, Value};

pub(crate) async fn resolve(
    state: &ServerState,
    key: &str,
    request: &mut Value,
) -> Result<(), String> {
    let owner = crate::access_control::hash_secret(key);
    let Some(input) = request["input"].as_array_mut() else {
        return Ok(());
    };
    for item in input {
        let Some(parts) = item.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for part in parts {
            let Some(id) = part["file_id"].as_str() else {
                continue;
            };
            let record = crate::file_api::read_record_for_owner(&state.file_root, id, &owner)
                .await
                .map_err(|_| "文件不存在或无权访问".to_string())?;
            let data = crate::file_api::read_file_as_data_url(&state.file_root, id, &owner)
                .await
                .map_err(|_| "读取文件失败".to_string())?;
            match part["type"].as_str() {
                Some("input_image") => {
                    part["image_url"] = json!(data);
                }
                Some("input_file") => {
                    part["file_data"] = json!(data);
                    part["filename"] = json!(record.filename);
                }
                _ => return Err("此输入类型不能引用 file_id".into()),
            }
            part.as_object_mut()
                .ok_or("输入内容必须是对象")?
                .remove("file_id");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn resolving_files_preserves_tool_and_reasoning_history_without_content() {
        let state = crate::protocol_api_tests::state();
        let mut request = serde_json::json!({"input":[
            {"type":"reasoning","encrypted_content":"opaque","summary":[]},
            {"type":"function_call","name":"result","call_id":"call","arguments":"{}"},
            {"type":"function_call_output","call_id":"call","output":"OK"}
        ]});
        let before = request.clone();
        super::resolve(&state, "key", &mut request).await.unwrap();
        assert_eq!(request, before);
    }
}
