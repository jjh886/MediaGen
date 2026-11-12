use crate::ServerState;
use serde_json::{json, Value};

pub(crate) async fn resolve(
    state: &ServerState,
    key: &str,
    request: &mut Value,
    capabilities: &Value,
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
            if capabilities["attachment_transport"] == "workspace" {
                if !matches!(part["type"].as_str(), Some("input_file" | "input_image")) {
                    return Err("此输入类型不能引用 file_id".into());
                }
                let mime = if record.mime_type.is_empty() {
                    crate::file_api::mime_from_filename(&record.filename).to_string()
                } else {
                    record.mime_type
                };
                let path = crate::file_api::content_path(&state.file_root, id);
                let kind = part["type"].clone();
                let detail = part.get("detail").cloned();
                *part = json!({"type":kind,"file_path":path,"filename":record.filename,"mime_type":mime,"size":record.bytes});
                if let Some(detail) = detail {
                    part["detail"] = detail;
                }
                continue;
            }
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

/// 只有宿主授权后的文件引用可到达这里；复制到独立任务目录，插件写入不会修改源文件。
pub(crate) async fn materialize(
    state: &ServerState,
    request: &mut Value,
    workspace: &std::path::Path,
) -> Result<(), String> {
    use std::collections::HashMap;
    let mut copies: HashMap<std::path::PathBuf, std::path::PathBuf> = HashMap::new();
    let Some(input) = request["input"].as_array_mut() else {
        return Ok(());
    };
    for item in input {
        let Some(parts) = item.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for part in parts {
            let Some(source) = part["file_path"].as_str() else {
                continue;
            };
            let source = tokio::fs::canonicalize(source)
                .await
                .map_err(|_| "附件文件不存在")?;
            let root = tokio::fs::canonicalize(&state.file_root)
                .await
                .map_err(|_| "文件目录不可读")?;
            if source.parent() != Some(root.as_path()) {
                return Err("附件引用不在授权文件目录".into());
            }
            let size = tokio::fs::metadata(&source)
                .await
                .map_err(|_| "附件文件不可读")?
                .len();
            if Some(size) != part["size"].as_u64() {
                return Err("附件大小已变化".into());
            }
            let target = if let Some(path) = copies.get(&source) {
                path.clone()
            } else {
                let temporary =
                    tempfile::NamedTempFile::new_in(workspace).map_err(|_| "创建附件副本失败")?;
                let copied = tokio::fs::copy(&source, temporary.path())
                    .await
                    .map_err(|_| "复制附件失败")?;
                if copied != size {
                    return Err("附件复制不完整".into());
                }
                let target = workspace.join(source.file_name().ok_or("附件文件名无效")?);
                tokio::fs::rename(temporary.path(), &target)
                    .await
                    .map_err(|_| "保存附件副本失败")?;
                copies.insert(source, target.clone());
                target
            };
            part["file_path"] = json!(target);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[actix_web::test]
    async fn novel_attachment_preserves_filename_mime_and_bytes_across_public_protocols() {
        use actix_web::{test, App};
        use base64::prelude::*;
        use serde_json::{json, Value};
        let state = crate::protocol_api_tests::state();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let filename = "在？遇到困难了？来买个外挂？(3).txt";
        let content = "第一章\n人物说：你好。\r\n第二段：中文标点？";
        let body = format!("--files\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nuser_data\r\n--files\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: text/plain\r\n\r\n{content}\r\n--files--\r\n");
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/v1/files")
                .insert_header(("authorization", "Bearer key-a"))
                .insert_header(("content-type", "multipart/form-data; boundary=files"))
                .set_payload(body)
                .to_request(),
        )
        .await;
        assert!(response.status().is_success());
        let uploaded: Value = test::read_body_json(response).await;
        assert_eq!(uploaded["filename"], filename);
        assert_eq!(uploaded["bytes"], content.len());
        let encoded = BASE64_STANDARD.encode(content);
        let data_url = format!("data:text/plain;base64,{encoded}");
        let chat = crate::protocol_request::chat_to_responses(&json!({"model":"text-model","messages":[{"role":"user","content":[{"type":"file","file":{"file_id":uploaded["id"]}}]}]})).unwrap();
        for mut request in [
            chat,
            json!({"input":[{"role":"user","content":[{"type":"input_file","file_id":uploaded["id"]}]}]}),
            json!({"input":[{"role":"user","content":[{"type":"input_file","filename":filename,"file_data":encoded}]}]}),
            json!({"input":[{"role":"user","content":[{"type":"input_file","filename":filename,"file_data":data_url}]}]}),
        ] {
            super::resolve(&state, "key-a", &mut request, &json!({}))
                .await
                .unwrap();
            crate::protocol_attachments::prepare(
                &state,
                "key-a",
                &mut request,
                &json!({"input_modalities":["file"]}),
            )
            .await
            .unwrap();
            let part = &request["input"][0]["content"][0];
            assert_eq!(part["filename"], filename);
            assert!(part.get("file_id").is_none());
            let (mime, bytes) = crate::protocol_attachments::decode(part).unwrap().unwrap();
            assert_eq!(mime, "text/plain");
            assert_eq!(bytes, content.as_bytes());
        }
    }

    #[tokio::test]
    async fn resolving_files_preserves_tool_and_reasoning_history_without_content() {
        let state = crate::protocol_api_tests::state();
        let mut request = serde_json::json!({"input":[
            {"type":"reasoning","encrypted_content":"opaque","summary":[]},
            {"type":"function_call","name":"result","call_id":"call","arguments":"{}"},
            {"type":"function_call_output","call_id":"call","output":"OK"}
        ]});
        let before = request.clone();
        super::resolve(&state, "key", &mut request, &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(request, before);
    }
}
