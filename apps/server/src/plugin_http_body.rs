use super::*;

pub(super) fn apply_body(
    request: reqwest::RequestBuilder,
    params: &Value,
    body: &Value,
) -> Result<reqwest::RequestBuilder, String> {
    match params["body_format"].as_str().unwrap_or("json") {
        "multipart" => Ok(request.multipart(multipart_form(body)?)),
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

pub(super) fn multipart_form(body: &Value) -> Result<reqwest::multipart::Form, String> {
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
    Ok(form)
}

// 每次重试重新打开文件，避免复用已消费的流；路径权限由现有宿主策略校验。
pub(super) async fn file_body(
    params: &Value,
) -> Result<
    (
        futures_util::stream::BoxStream<'static, Result<bytes::Bytes, std::io::Error>>,
        u64,
    ),
    String,
> {
    use tokio::io::AsyncReadExt;
    let file = tokio::fs::File::open(required_str(params, "body_file")?)
        .await
        .map_err(|_| "上传文件不可读")?;
    let metadata = file.metadata().await.map_err(|_| "上传文件不可读")?;
    let size = metadata.len();
    if !metadata.is_file()
        || params
            .get("body_file_size")
            .is_some_and(|value| value.as_u64() != Some(size))
    {
        return Err("上传文件大小不一致或不是普通文件".into());
    }
    let stream =
        futures_util::stream::try_unfold((file, size), |(mut file, remaining)| async move {
            if remaining == 0 {
                return Ok(None);
            }
            let mut chunk = vec![0u8; remaining.min(64 * 1024) as usize];
            let read = file.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "上传文件读取不完整",
                ));
            }
            chunk.truncate(read);
            Ok(Some((
                bytes::Bytes::from(chunk),
                (file, remaining - read as u64),
            )))
        })
        .boxed();
    Ok((stream, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn file_stream_is_bounded_and_accepts_large_files_without_base64() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large.bin");
        let file = std::fs::File::create(&path).unwrap();
        let size = 51 * 1024 * 1024;
        file.set_len(size).unwrap();
        let params = json!({"body_file":path,"body_file_size":size});
        let (mut stream, actual) = file_body(&params).await.unwrap();
        assert_eq!(actual, size);
        let mut received = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            assert!(chunk.len() <= 64 * 1024);
            received += chunk.len() as u64;
        }
        assert_eq!(received, size);
        assert!(
            file_body(&json!({"body_file":path,"body_file_size":size+1}))
                .await
                .is_err()
        );
        let (mut truncated, _) = file_body(&params).await.unwrap();
        file.set_len(0).unwrap();
        assert!(truncated.next().await.unwrap().is_err());
        assert!(file_body(&json!({"body_file":root.path()})).await.is_err());
    }
}
