use super::*;

pub(super) fn apply_body(
    request: reqwest::RequestBuilder,
    params: &Value,
    body: &Value,
) -> Result<reqwest::RequestBuilder, String> {
    match params["body_format"].as_str().unwrap_or("json") {
        "multipart" => {
            let mut form = reqwest::multipart::Form::new();
            for (name, value) in body.as_object().ok_or("multipart body 必须是对象")? {
                if let Some(encoded) = value["base64"].as_str() {
                    let bytes = BASE64_STANDARD
                        .decode(encoded)
                        .map_err(|_| "multipart 文件编码无效".to_string())?;
                    if bytes.len() > MAX_FILE_BYTES as usize {
                        return Err("上传文件过大".into());
                    }
                    let part = reqwest::multipart::Part::bytes(bytes)
                        .file_name(value["filename"].as_str().unwrap_or("upload").to_string())
                        .mime_str(
                            value["mime_type"]
                                .as_str()
                                .unwrap_or("application/octet-stream"),
                        )
                        .map_err(|_| "文件 MIME 类型无效".to_string())?;
                    form = form.part(name.clone(), part);
                } else {
                    form = form.text(name.clone(), value_to_string(value));
                }
            }
            Ok(request.multipart(form))
        }
        "form" | "x-www-form-urlencoded" => {
            let object = body
                .as_object()
                .ok_or_else(|| "form body 必须是对象".to_string())?;
            let pairs = object
                .iter()
                .map(|(key, value)| (key.clone(), value_to_string(value)))
                .collect::<Vec<_>>();
            Ok(request.form(&pairs))
        }
        "text" | "raw" => Ok(request.body(value_to_string(body))),
        "base64" => {
            let encoded = body
                .as_str()
                .ok_or_else(|| "base64 body 必须是字符串".to_string())?;
            let bytes = BASE64_STANDARD
                .decode(encoded)
                .map_err(|err| format!("base64 body 解码失败: {err}"))?;
            Ok(request.body(bytes))
        }
        _ => Ok(request.json(body)),
    }
}
