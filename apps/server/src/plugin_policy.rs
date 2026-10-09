use super::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct Policy {
    actions: HashSet<String>,
    hosts: HashSet<String>,
    fields: HashSet<String>,
    paths: Arc<Mutex<Paths>>,
}
#[derive(Default)]
struct Paths {
    roots: Vec<PathBuf>,
    references: Vec<PathBuf>,
}
fn strings(value: &Value) -> HashSet<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}
impl Policy {
    pub(super) fn new(value: &Value) -> Self {
        Self {
            actions: strings(&value["actions"]),
            hosts: strings(&value["network_hosts"]),
            fields: strings(&value["context_fields"]),
            paths: Default::default(),
        }
    }
    pub(super) fn visible_context(&self, context: Value) -> Value {
        let mut paths = self.paths.lock().unwrap_or_else(|e| e.into_inner());
        paths.roots = ["profile_dir", "workspace_dir"]
            .iter()
            .filter_map(|key| context[key].as_str())
            .filter_map(|path| canonical(Path::new(path)).ok())
            .collect();
        paths.references = ["ref_image_locals", "ref_video_locals", "ref_audio_locals"]
            .iter()
            .flat_map(|key| context[key].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .filter_map(|path| canonical(Path::new(path)).ok())
            .collect();
        Value::Object(
            context
                .as_object()
                .into_iter()
                .flat_map(|object| object.iter())
                .filter(|(key, _)| {
                    self.fields.contains(*key)
                        && !matches!(
                            key.as_str(),
                            "account"
                                | "account_cookies"
                                | "api_key"
                                | "bearer_token"
                                | "secret_key"
                                | "home_dir"
                        )
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }
    pub(super) fn validate(&self, action: &str, params: &Value) -> Result<(), String> {
        if matches!(action, "checkpoint" | "stream.emit") {
            return Ok(());
        }
        let result = self.check(action, params);
        if let Err(reason) = &result {
            tracing::warn!(reason = reason.as_str(), "插件能力校验拒绝操作");
        }
        result
    }
    fn check(&self, action: &str, params: &Value) -> Result<(), String> {
        if !self.actions.contains(action) {
            return Err("插件未声明此原子能力".into());
        }
        if let Some(url) = params["url"].as_str() {
            let mut url = reqwest::Url::parse(url).map_err(|_| "插件网络地址无效")?;
            if action == "http.websocket_open" {
                let scheme = match url.scheme() {
                    "wss" => "https",
                    "ws" => "http",
                    _ => return Err("WebSocket 协议无效".into()),
                };
                url.set_scheme(scheme).map_err(|_| "WebSocket 协议无效")?;
            }
            if !self.url_allowed(&url) {
                return Err("插件网络域名不在声明范围内".into());
            }
        }
        for key in ["path", "body_file"] {
            if let Some(path) = params[key].as_str().filter(|value| !value.is_empty()) {
                let path = canonical(Path::new(path))?;
                let paths = self.paths.lock().unwrap_or_else(|e| e.into_inner());
                let writing = action.starts_with("file.write_") || action == "http.download";
                if !paths.roots.iter().any(|root| path.starts_with(root))
                    && (writing || !paths.references.contains(&path))
                {
                    return Err("插件文件路径不在当前账号或任务范围内".into());
                }
            }
        }
        Ok(())
    }
    pub(super) fn url_allowed(&self, url: &reqwest::Url) -> bool {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some_and(|host| {
                self.hosts
                    .iter()
                    .any(|allowed| host == allowed || host.ends_with(&format!(".{allowed}")))
            })
    }
}
fn canonical(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("插件文件路径必须为无越界的绝对路径".into());
    }
    let mut existing = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(mut root) => {
                for name in missing.into_iter().rev() {
                    root.push(name);
                }
                return Ok(root);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // 悬空链接不得作为可写的新目录，防止写入时越界。
                if std::fs::symlink_metadata(&existing).is_ok() {
                    return Err("插件文件路径包含无效链接".into());
                }
                missing.push(
                    existing
                        .file_name()
                        .ok_or("插件文件路径无效")?
                        .to_os_string(),
                );
                if !existing.pop() {
                    return Err("插件文件路径无效".into());
                }
            }
            Err(_) => return Err("插件文件路径不可访问".into()),
        }
    }
}

impl ServerPluginHost {
    pub(super) fn native_redirect_policy(&self, params: &Value) -> reqwest::redirect::Policy {
        if params["follow_redirects"] == false {
            return reqwest::redirect::Policy::none();
        }
        let policy = self.policy.clone();
        reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10
                || policy
                    .as_ref()
                    .is_some_and(|policy| !policy.url_allowed(attempt.url()))
            {
                attempt.error("插件重定向被安全策略拒绝")
            } else {
                attempt.follow()
            }
        })
    }
    pub(super) fn profile_redirect_policy(&self, params: &Value) -> wreq::redirect::Policy {
        if params["follow_redirects"] == false {
            return wreq::redirect::Policy::none();
        }
        let policy = self.policy.clone();
        wreq::redirect::Policy::custom(move |attempt| {
            if attempt.previous.len() >= 10
                || policy.as_ref().is_some_and(|policy| {
                    reqwest::Url::parse(&attempt.uri.to_string())
                        .map_or(true, |url| !policy.url_allowed(&url))
                })
            {
                attempt.error("插件重定向被安全策略拒绝")
            } else {
                attempt.follow()
            }
        })
    }
    pub(super) fn validate_policy(&self, action: &str, params: &Value) -> Result<(), String> {
        if let Some(policy) = &self.policy {
            policy.validate(action, params)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn verification_storage_works_before_runtime_context_is_bound() {
        let root = tempfile::tempdir().unwrap();
        let cipher = Arc::new(crate::generation_queue::CredentialCipher::new(root.path()).unwrap());
        let manifest: mediagen_core::PluginManifest = serde_json::from_value(json!({
            "platform_id":"test", "display_name":"Test",
            "protected_json_files":["verification.json"],
            "capabilities": {
                "actions":["file.write_json","file.read_json"],
                "context_fields":["profile_dir","prompt"]
            }
        }))
        .unwrap();
        let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default())
            .with_storage(cipher, root.path().to_str().unwrap(), &manifest);
        let path = root.path().join("verification.json");
        host.execute(
            "file.write_json",
            &json!({"path":path,"value":{"challenge":"private-value"}}),
        )
        .await
        .unwrap();
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("private-value"));
        assert_eq!(
            host.execute("file.read_json", &json!({"path":path}))
                .await
                .unwrap()["challenge"],
            "private-value"
        );
        let result = mediagen_plugin_runtime::run_js_step(
            "function main(ctx){return {prompt:ctx.prompt, leaked:!!ctx.account}}".into(),
            json!({"profile_dir":root.path(),"prompt":"hello","account":{"token":"private"}}),
            host,
        )
        .await
        .unwrap();
        assert_eq!(result["prompt"], "hello");
        assert_eq!(result["leaked"], false);
    }
    #[test]
    fn declared_capabilities_hide_secrets_and_bound_network_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let reference = outside.path().join("reference.png");
        std::fs::write(&reference, b"image").unwrap();
        let policy = Policy::new(
            &json!({"actions":["http.request","file.read_base64","file.write_base64"],"network_hosts":["example.com"],"context_fields":["profile_dir","prompt","account"]}),
        );
        let visible = policy.visible_context(json!({"profile_dir":dir.path(),"prompt":"hello","account":{"api_key":"private"},"ref_image_locals":[reference]}));
        assert!(visible.get("account").is_none());
        assert_eq!(visible["prompt"], "hello");
        assert!(policy.validate("process.run", &json!({})).is_err());
        assert!(policy
            .validate("http.request", &json!({"url":"https://cdn.example.com/a"}))
            .is_ok());
        for url in [
            "https://example.com.attacker.test/a",
            "https://127.0.0.1/a",
            "file:///private/data",
        ] {
            assert!(policy
                .validate("http.request", &json!({"url":url}))
                .is_err());
        }
        assert!(policy
            .validate(
                "file.write_base64",
                &json!({"path":dir.path().join("new/a.bin")})
            )
            .is_ok());
        assert!(policy
            .validate("file.read_base64", &json!({"path":reference}))
            .is_ok());
        assert!(policy
            .validate("file.write_base64", &json!({"path":reference}))
            .is_err());
        assert!(policy
            .validate(
                "file.read_base64",
                &json!({"path":dir.path().join("../secret")})
            )
            .is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("escape");
            std::os::unix::fs::symlink(outside.path(), &link).unwrap();
            assert!(policy
                .validate("file.write_base64", &json!({"path":link.join("new.bin")}))
                .is_err());
        }
    }
}
