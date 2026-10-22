use crate::ServerState;
use serde_json::Value;

fn path(state: &ServerState, id: &str) -> std::path::PathBuf {
    state
        .file_root
        .join("generation-results")
        .join(format!("{}.json", crate::access_control::hash_secret(id)))
}
pub(crate) async fn save(state: &ServerState, id: &str, value: &Value) -> Result<(), String> {
    let json = serde_json::to_string(value).map_err(|_| "图片结果编码失败")?;
    let encrypted = state.credential_cipher.encrypt(&json)?;
    crate::account_login_credentials::write_private_file(&path(state, id), encrypted.as_bytes())
        .await
}
pub(crate) async fn read(state: &ServerState, id: &str) -> Result<Option<Value>, String> {
    let bytes = match tokio::fs::read(path(state, id)).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("读取图片结果失败".into()),
    };
    let encoded = std::str::from_utf8(&bytes).map_err(|_| "图片结果编码无效")?;
    let json = state.credential_cipher.decrypt(encoded)?;
    serde_json::from_str(&json)
        .map(Some)
        .map_err(|_| "图片结果格式无效".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn queued_images_are_private_encrypted_and_survive_new_state() {
        let root = std::env::temp_dir().join(format!("queued-result-{}", uuid::Uuid::new_v4()));
        let state = crate::test_fixtures::server_state(
            root.clone(),
            mediagen_plugin_runtime::PluginLoader::empty(),
        );
        let value = serde_json::json!({"created":123,"data":[{"b64_json":"private-image-bytes"}]});
        assert!(read(&state, "unknown").await.unwrap().is_none());
        save(&state, "task", &value).await.unwrap();
        let raw = tokio::fs::read_to_string(path(&state, "task"))
            .await
            .unwrap();
        assert!(!raw.contains("private-image-bytes"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path(&state, "task"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let mut restarted = crate::test_fixtures::server_state(
            root.clone(),
            mediagen_plugin_runtime::PluginLoader::empty(),
        );
        restarted.credential_cipher = state.credential_cipher.clone();
        assert_eq!(read(&restarted, "task").await.unwrap(), Some(value));
        tokio::fs::write(path(&state, "task"), b"corrupt")
            .await
            .unwrap();
        assert!(read(&state, "task").await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
