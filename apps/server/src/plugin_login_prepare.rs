use mediagen_browser_gateway::{BrowserSessionCreateRequest, BrowserSessionPurpose};
use mediagen_core::PluginManifest;
use serde_json::json;

use crate::{server_plugin_host::ServerPluginHost, ServerState};

/// 登录前的通用插件钩子：设备码、授权地址等平台协议全部由插件执行。
pub(crate) async fn prepare(
    state: &ServerState,
    manifest: &PluginManifest,
    request: &mut BrowserSessionCreateRequest,
) -> Result<(), String> {
    if !matches!(
        request.purpose,
        BrowserSessionPurpose::AddAccount | BrowserSessionPurpose::ReloginAccount
    ) {
        return Ok(());
    }
    let Some(config) = manifest.extra.get("login_prepare") else {
        return Ok(());
    };
    let references: Vec<String> = serde_json::from_value(config["script_files"].clone())
        .map_err(|_| "登录准备脚本配置无效")?;
    if request
        .profile_dir
        .as_deref()
        .is_none_or(|path| path.trim().is_empty())
    {
        request.profile_dir = Some(
            state
                .browser_gateway
                .profile_root()
                .join(format!("login-{}", uuid::Uuid::new_v4()))
                .to_string_lossy()
                .into_owned(),
        );
    }
    let profile = request.profile_dir.as_deref().ok_or("账号目录缺失")?;
    let directory = state
        .plugin_loader
        .get_plugin_dir(&manifest.platform_id)
        .ok_or("插件不存在")?;
    let script = crate::plugin_scripts::read_plugin_scripts(directory, &references)?;
    let host = ServerPluginHost::new_with_metadata(
        request
            .metadata
            .get("proxy_server")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into(),
        &request.metadata,
    )
    .with_storage(state.credential_cipher.clone(), profile, manifest)
    .with_http_profile(manifest);
    let value = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        mediagen_plugin_runtime::run_js_step(
            script,
            json!({"profile_dir":profile,"platform":manifest.platform_id}),
            host,
        ),
    )
    .await
    .map_err(|_| "登录准备超时")??;
    let url = value["start_url"].as_str().ok_or("插件没有返回登录地址")?;
    let parsed = reqwest::Url::parse(url).map_err(|_| "登录地址无效")?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() || !parsed.username().is_empty() {
        return Err("登录地址必须使用 HTTPS".into());
    }
    request.start_url = url.into();
    if let Some(hint) = value["instructions"].as_str() {
        request
            .metadata
            .insert("login_instructions".into(), json!(hint));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn login_hook_uses_private_storage_and_only_exports_public_instructions() {
        let root = std::env::temp_dir().join(format!("login-hook-{}", uuid::Uuid::new_v4()));
        let dir = root.join("plugins/test/scripts");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prepare.js"), "function main(ctx){host.call('file.write_json',{path:ctx.profile_dir+'/pending.json',value:{secret:'private-device-code'}});return {start_url:'https://auth.example.test/activate',instructions:'输入 ABCD'}}").unwrap();
        std::fs::write(root.join("plugins/test/manifest.json"),json!({"platform_id":"test","display_name":"Test","models":[],"flows":{},"protected_json_files":["pending.json"],"login_prepare":{"script_files":["prepare.js"]}}).to_string()).unwrap();
        let loader =
            mediagen_plugin_runtime::PluginLoader::load_from(&root.join("plugins")).unwrap();
        let manifest = loader.get_plugin("test").unwrap().clone();
        let state = crate::test_fixtures::server_state(root.clone(), loader);
        let mut request: BrowserSessionCreateRequest = serde_json::from_value(
            json!({"platform":"test","purpose":"add_account","start_url":"https://example.test"}),
        )
        .unwrap();
        prepare(&state, &manifest, &mut request).await.unwrap();
        assert_eq!(request.start_url, "https://auth.example.test/activate");
        assert_eq!(request.metadata["login_instructions"], "输入 ABCD");
        assert!(!serde_json::to_string(&request.metadata)
            .unwrap()
            .contains("private-device-code"));
        let saved = std::fs::read_to_string(
            std::path::Path::new(request.profile_dir.as_ref().unwrap()).join("pending.json"),
        )
        .unwrap();
        assert!(!saved.contains("private-device-code"));
        std::fs::write(
            dir.join("prepare.js"),
            "function main(){return {start_url:'http://insecure.test'}}",
        )
        .unwrap();
        assert!(prepare(&state, &manifest, &mut request)
            .await
            .unwrap_err()
            .contains("HTTPS"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
