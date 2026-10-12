use super::*;

pub(super) fn uuid_v1(params: &Value) -> Result<Value, String> {
    let node: [u8; 6] = decode(params, "node_base64", 6)?
        .try_into()
        .map_err(|_| "UUID 节点标识必须为 6 字节")?;
    Ok(json!({"value": uuid::Uuid::now_v1(&node).to_string()}))
}

fn decode(value: &Value, field: &str, limit: usize) -> Result<Vec<u8>, String> {
    let encoded = value[field].as_str().ok_or("缺少二进制参数")?;
    if encoded.len() > limit.div_ceil(3) * 4 {
        return Err("二进制参数过大".into());
    }
    let bytes = BASE64_STANDARD
        .decode(encoded)
        .map_err(|_| "二进制编码无效")?;
    if bytes.len() > limit {
        return Err("二进制参数过大".into());
    }
    Ok(bytes)
}

pub(super) fn crypto(action: &str, params: &Value) -> Result<Value, String> {
    let data = decode(params, "data_base64", MAX_FILE_BYTES as usize)?;
    let hash = match action {
        "crypto.digest_bytes" => match params["algorithm"].as_str() {
            Some("md5") => md5::Md5::digest(&data).to_vec(),
            Some("sha256") => Sha256::digest(&data).to_vec(),
            Some("crc32") => crc32fast::hash(&data).to_be_bytes().to_vec(),
            _ => return Err("摘要算法不支持".into()),
        },
        "crypto.hmac_sha256" => {
            use hmac::{Hmac, Mac};
            let key = decode(params, "key_base64", 4096)?;
            let mut mac = Hmac::<Sha256>::new_from_slice(&key).map_err(|_| "HMAC 密钥无效")?;
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        _ => return Err("二进制操作不支持".into()),
    };
    Ok(json!({"value": BASE64_STANDARD.encode(hash)}))
}

impl ServerPluginHost {
    pub(super) async fn download(&self, params: &Value) -> Result<Value, String> {
        let path = required_str(params, "path")?;
        let mut request = params.clone();
        request["response_body_format"] = json!("base64");
        let response = self.http_request(&request, false).await?;
        if response["ok"] != true {
            return Err("文件下载失败".into());
        }
        self.write_base64(&json!({"path":path,"base64":response["body_base64"]}))
            .await
    }

    pub(super) async fn write_base64(&self, params: &Value) -> Result<Value, String> {
        let path = PathBuf::from(required_str(params, "path")?);
        let bytes = decode(params, "base64", MAX_FILE_BYTES as usize)?;
        crate::account_login_credentials::write_private_file(&path, &bytes).await?;
        Ok(json!({"path":path,"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}))
    }

    pub(super) async fn inspect_media(&self, params: &Value) -> Result<Value, String> {
        use tokio::io::AsyncReadExt;
        let path = tokio::fs::canonicalize(required_str(params, "path")?)
            .await
            .map_err(|_| "媒体文件不存在")?;
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|_| "媒体文件不可读")?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err("媒体附件必须是不超过 50 MiB 的本地文件".into());
        }
        let binary = mediagen_core::resolve_ffprobe_binary();
        let mut child = tokio::process::Command::new(binary)
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-show_entries",
                "format=duration,format_name:stream=codec_type:stream_disposition=attached_pic",
                "-of",
                "json",
            ])
            .arg(&path)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|_| "媒体信息检查需要可用的 FFprobe")?;
        let output = child.stdout.take().ok_or("媒体信息输出不可用")?;
        let execution = async {
            let mut bytes = Vec::new();
            output
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "媒体信息读取失败")?;
            if bytes.len() > 1024 * 1024 {
                return Err("媒体信息过大".to_string());
            }
            if !child
                .wait()
                .await
                .map_err(|_| "媒体检查进程失败")?
                .success()
            {
                return Err("无法读取媒体附件信息".into());
            }
            let info: Value = serde_json::from_slice(&bytes).map_err(|_| "媒体信息无效")?;
            let streams = info["streams"].as_array().ok_or("媒体流信息缺失")?;
            Ok(
                json!({"size":metadata.len(),"container":info["format"]["format_name"],
                "duration":info["format"]["duration"].as_str().and_then(|s|s.parse::<f64>().ok()).filter(|n|n.is_finite() && *n >= 0.0),
                "has_audio":streams.iter().any(|s|s["codec_type"]=="audio"),
                "has_video":streams.iter().any(|s|s["codec_type"]=="video" && s["disposition"]["attached_pic"]!=1)}),
            )
        };
        tokio::time::timeout(Duration::from_secs(30), execution)
            .await
            .map_err(|_| "媒体信息检查超时")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn time_uuid_preserves_node_version_and_uniqueness() {
        let params = json!({"node_base64":BASE64_STANDARD.encode([1,2,3,4,5,6])});
        let first = uuid_v1(&params).unwrap();
        let second = uuid_v1(&params).unwrap();
        let id = uuid::Uuid::parse_str(first["value"].as_str().unwrap()).unwrap();
        assert_eq!(id.get_version_num(), 1);
        assert_eq!(&id.as_bytes()[10..], &[1, 2, 3, 4, 5, 6]);
        assert_ne!(first, second);
        for input in [
            json!({}),
            json!({"node_base64":"!"}),
            json!({"node_base64":"AQID"}),
            json!({"node_base64":"AQIDBAUGBw=="}),
        ] {
            assert!(uuid_v1(&input).is_err());
        }
    }
    #[test]
    fn checksum_vectors_and_limits() {
        let crc = crypto(
            "crypto.digest_bytes",
            &json!({"algorithm":"crc32","data_base64":BASE64_STANDARD.encode(b"123456789")}),
        )
        .unwrap();
        assert_eq!(
            BASE64_STANDARD
                .decode(crc["value"].as_str().unwrap())
                .unwrap(),
            [0xcb, 0xf4, 0x39, 0x26]
        );
        let result = crypto("crypto.hmac_sha256", &json!({"key_base64":BASE64_STANDARD.encode([0x0b;20]),"data_base64":BASE64_STANDARD.encode(b"Hi There")})).unwrap();
        assert_eq!(
            BASE64_STANDARD
                .decode(result["value"].as_str().unwrap())
                .unwrap()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert!(decode(&json!({"base64":"YWJj"}), "base64", 2).is_err());
        assert!(crypto(
            "crypto.digest_bytes",
            &json!({"algorithm":"unknown","data_base64":""})
        )
        .is_err());
    }

    #[tokio::test]
    async fn binary_files_preserve_bytes_ranges_and_mime() {
        let work = tempfile::tempdir().unwrap();
        let gif = work.path().join("attachment.gif");
        let gif_data = "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
        let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
        assert!(
            host.call("file.write_base64", &json!({"path":gif,"base64":gif_data}))
                .await
                .success
        );
        let image = host.call("file.inspect_image", &json!({"path":gif})).await;
        assert!(image.success, "{:?}", image.error);
        assert_eq!(image.data["mime"], "image/gif");
        assert_eq!(image.data["width"], 1);
        let path = work.path().join("attachment.bin");
        let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
        let result = host
            .call(
                "file.write_base64",
                &json!({"path":path,"base64":"YWJjZGVm"}),
            )
            .await;
        assert!(result.success);
        assert_eq!(result.data["size"], 6);
        let chunk = host
            .call(
                "file.read_base64",
                &json!({"path":path,"offset":2,"length":3}),
            )
            .await;
        assert!(chunk.success);
        assert_eq!(chunk.data["base64"], "Y2Rl");
        assert!(
            !host
                .call(
                    "file.read_base64",
                    &json!({"path":path,"offset":5,"length":2})
                )
                .await
                .success
        );
        assert!(
            !host
                .call(
                    "file.write_base64",
                    &json!({"path":path,"base64":"invalid!"})
                )
                .await
                .success
        );
    }
}
