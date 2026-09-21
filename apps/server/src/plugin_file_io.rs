use super::*;

impl ServerPluginHost {
    pub(crate) fn with_storage(
        mut self,
        cipher: std::sync::Arc<crate::generation_queue::CredentialCipher>,
        profile_dir: &str,
        manifest: &mediagen_core::PluginManifest,
    ) -> Self {
        self.credential_cipher = Some(cipher);
        self.profile_dir = PathBuf::from(profile_dir);
        self.protected_files = manifest
            .extra
            .get("protected_json_files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| !name.contains('/') && !name.contains('\\'))
            .map(|name| self.profile_dir.join(name))
            .collect();
        self
    }
    pub(crate) async fn safe_to_retry(&self) -> bool {
        if self.emitted.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        self.checkpoint(&json!({}))
            .await
            .is_ok_and(|value| match value["phase"].as_str() {
                Some("ready") => true,
                None | Some("preparing") => {
                    !self.submitted.load(std::sync::atomic::Ordering::Relaxed)
                }
                _ => false,
            })
    }
    pub(super) async fn read_json(&self, params: &Value) -> Result<Value, String> {
        let path = required_str(params, "path")?;
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|err| format!("file.read_json 读取失败 {path}: {err}"))?;
        let stored: Value = serde_json::from_str(&content).map_err(|_| "JSON 文件格式无效")?;
        if stored["storage_format"] == "mediagen.encrypted.v1" {
            let cipher = self.credential_cipher.as_ref().ok_or("安全存储未配置")?;
            let plaintext = cipher.decrypt(stored["ciphertext"].as_str().ok_or("凭据密文缺失")?)?;
            return serde_json::from_str(&plaintext).map_err(|_| "凭据格式无效".into());
        }
        if self.protected_files.contains(&PathBuf::from(&path)) {
            self.write_json(&json!({"path":path,"value":stored}))
                .await?;
        }
        Ok(stored)
    }

    pub(super) async fn write_json(&self, params: &Value) -> Result<Value, String> {
        let path = PathBuf::from(required_str(params, "path")?);
        let value = params.get("value").ok_or("file.write_json 缺少 value")?;
        let protected = self.protected_files.contains(&path) || params["encrypted"] == true;
        let stored = if protected {
            let cipher = self.credential_cipher.as_ref().ok_or("安全存储未配置")?;
            json!({"storage_format":"mediagen.encrypted.v1","ciphertext":cipher.encrypt(&value.to_string())?})
        } else {
            value.clone()
        };
        let content = serde_json::to_vec(&stored).map_err(|_| "JSON 编码失败")?;
        crate::account_login_credentials::write_private_file(&path, &content).await?;
        Ok(json!({"path":path.to_string_lossy(),"bytes":content.len()}))
    }

    pub(super) async fn checkpoint(&self, params: &Value) -> Result<Value, String> {
        if self.request_id.is_empty() || self.profile_dir.as_os_str().is_empty() {
            let mut checkpoint = self
                .checkpoint_memory
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(value) = params.get("value") {
                *checkpoint = value.clone();
            }
            return Ok(checkpoint.clone());
        }
        let key = format!("{:x}", Sha256::digest(self.request_id.as_bytes()));
        let path = self
            .profile_dir
            .join("checkpoints")
            .join(format!("{key}.json"));
        if let Some(value) = params.get("value") {
            if value.to_string().len() > 8 * 1024 * 1024 {
                return Err("检查点超过大小限制".into());
            }
            self.write_json(&json!({"path":path,"value":value,"encrypted":true}))
                .await?;
            return Ok(value.clone());
        }
        if !path.exists() {
            return Ok(json!({}));
        }
        self.read_json(&json!({"path":path})).await
    }

    pub(super) async fn read_base64(&self, params: &Value) -> Result<Value, String> {
        let path = required_str(params, "path")?;
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|err| format!("file.read_base64 找不到文件 {path}: {err}"))?;
        let max_bytes = params["max_bytes"].as_u64().unwrap_or(MAX_FILE_BYTES);
        if metadata.len() > max_bytes {
            return Err(format!(
                "file.read_base64 文件超过限制 {max_bytes} bytes: {path}"
            ));
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|err| err.to_string())?;
        Ok(json!({
            "path": path,
            "base64": BASE64_STANDARD.encode(&bytes),
            "mime": mime_type(&path),
            "size": bytes.len(),
        }))
    }

    pub(super) async fn inspect_image(&self, params: &Value) -> Result<Value, String> {
        let path = params["path"].as_str().unwrap_or("");
        let bytes = if let Some(encoded) = params["base64"].as_str() {
            if encoded.len() > (MAX_FILE_BYTES as usize * 4 / 3 + 4) {
                return Err("图片超过大小限制".into());
            }
            BASE64_STANDARD
                .decode(encoded)
                .map_err(|_| "图片编码无效")?
        } else {
            read_limited_file(path, params["max_bytes"].as_u64()).await?
        };
        let size = bytes.len();
        // 这里只读取图片头部尺寸，不解码整张像素图，避免大图额外占用内存。
        let reader = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|err| format!("file.inspect_image 无法识别图片 {path}: {err}"))?;
        let mime = reader
            .format()
            .map(|format| format.to_mime_type())
            .unwrap_or("application/octet-stream");
        let (width, height) = reader
            .into_dimensions()
            .map_err(|err| format!("file.inspect_image 读取尺寸失败 {path}: {err}"))?;
        Ok(
            json!({ "path": path, "width": width, "height": height, "size": size, "mime_type": mime }),
        )
    }
}
