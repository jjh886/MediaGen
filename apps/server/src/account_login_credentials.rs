use std::path::{Path, PathBuf};

use mediagen_browser_gateway::{BrowserCdpRequest, BrowserSession};
use mediagen_core::PluginManifest;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{account_login, ServerState};

const CONFIG_KEY: &str = "login_credential_capture";

#[derive(Debug, Deserialize)]
struct LoginCredentialCaptureConfig {
    #[serde(default)]
    script_file: String,
    #[serde(default)]
    script_files: Vec<String>,
    #[serde(default = "default_await_promise")]
    await_promise: bool,
    #[serde(default = "default_file_name")]
    file_name: String,
    #[serde(default)]
    cookie_urls: Vec<String>,
}

/// 登录成功后按插件声明采集凭据。Rust 只负责通用的脚本执行、Cookie 读取和安全落盘，
/// 凭据字段如何解析仍由声明该能力的插件脚本决定。
pub async fn capture_if_configured(
    state: &ServerState,
    session: &BrowserSession,
    manifest: &PluginManifest,
) -> Result<(), String> {
    let Some(config) = capture_config(manifest)? else {
        return Ok(());
    };
    validate_config(&config)?;

    let script_files = account_login::configured_script_files(
        config.script_file.clone(),
        config.script_files.clone(),
    )?;
    let auth =
        account_login::run_login_script_value(state, session, &script_files, config.await_promise)
            .await?;
    let cookies = capture_cookies(state, session, &config.cookie_urls).await?;
    let value = json!({
        "version": 1,
        "captured_at_unix_ms": unix_time_millis(),
        "auth": auth,
        "cookies": cookies,
    });
    write_private_json(&session.profile_dir, &config.file_name, &value).await
}

fn capture_config(
    manifest: &PluginManifest,
) -> Result<Option<LoginCredentialCaptureConfig>, String> {
    let Some(value) = manifest.extra.get(CONFIG_KEY) else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|err| format!("登录凭据采集配置解析失败: {err}"))
}

fn validate_config(config: &LoginCredentialCaptureConfig) -> Result<(), String> {
    let script_files = account_login::configured_script_files(
        config.script_file.clone(),
        config.script_files.clone(),
    )?;
    if script_files
        .iter()
        .any(|value| !crate::plugin_scripts::is_safe_script_reference(value))
    {
        return Err("登录凭据采集脚本文件名不安全".to_string());
    }
    if !safe_file_name(&config.file_name) {
        return Err("登录凭据文件名不安全".to_string());
    }
    for url in &config.cookie_urls {
        let normalized = url.trim().to_ascii_lowercase();
        if !normalized.starts_with("https://") && !normalized.starts_with("http://") {
            return Err(format!("Cookie 采集地址只支持 HTTP/HTTPS: {url}"));
        }
    }
    Ok(())
}

async fn capture_cookies(
    state: &ServerState,
    session: &BrowserSession,
    urls: &[String],
) -> Result<Value, String> {
    if urls.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let response = state
        .browser_gateway
        .send_cdp_command(
            &session.id,
            BrowserCdpRequest {
                method: "Network.getCookies".to_string(),
                params: json!({ "urls": urls }),
            },
        )
        .await
        .map_err(|err| format!("读取登录 Cookie 失败: {err}"))?;
    response
        .data
        .get("cookies")
        .cloned()
        .filter(Value::is_array)
        .ok_or_else(|| "读取登录 Cookie 失败：CDP 响应缺少 cookies".to_string())
}

async fn write_private_json(
    profile_dir: &str,
    file_name: &str,
    value: &Value,
) -> Result<(), String> {
    let path = PathBuf::from(profile_dir).join(file_name);
    let parent = path
        .parent()
        .ok_or_else(|| "登录凭据文件缺少父目录".to_string())?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|err| format!("创建账号目录失败: {err}"))?;
    let bytes = [
        serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?,
        b"\n".to_vec(),
    ]
    .concat();
    write_private_file(&path, &bytes).await
}

#[cfg(unix)]
async fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::AsyncWriteExt;

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .await
        .map_err(|err| format!("写入登录凭据失败: {err}"))?;
    file.write_all(bytes)
        .await
        .map_err(|err| format!("写入登录凭据失败: {err}"))?;
    file.flush()
        .await
        .map_err(|err| format!("刷新登录凭据失败: {err}"))?;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .await
        .map_err(|err| format!("设置登录凭据权限失败: {err}"))
}

#[cfg(not(unix))]
async fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    tokio::fs::write(path, bytes)
        .await
        .map_err(|err| format!("写入登录凭据失败: {err}"))
}

fn safe_file_name(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..")
}

fn unix_time_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn default_await_promise() -> bool {
    true
}

fn default_file_name() -> String {
    "auth.json".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_script_and_output_names() {
        let mut config = LoginCredentialCaptureConfig {
            script_file: "../capture.js".to_string(),
            script_files: Vec::new(),
            await_promise: true,
            file_name: "auth.json".to_string(),
            cookie_urls: vec!["https://example.test/".to_string()],
        };
        assert!(validate_config(&config).is_err());

        config.script_file = "capture.js".to_string();
        config.file_name = "nested/auth.json".to_string();
        assert!(validate_config(&config).is_err());
    }

    #[tokio::test]
    async fn writes_private_auth_file() {
        let dir = std::env::temp_dir().join(format!(
            "mediagen-login-credentials-{}",
            uuid::Uuid::new_v4()
        ));
        write_private_json(
            dir.to_str().unwrap(),
            "auth.json",
            &json!({ "auth": { "accessToken": "secret" } }),
        )
        .await
        .unwrap();

        let path = dir.join("auth.json");
        assert!(path.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
