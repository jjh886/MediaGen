use mediagen_core::ModelDef;
use mediagen_plugin_runtime::PluginLoader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub(crate) struct Catalog(
    pub Arc<Mutex<HashMap<String, CachedModels>>>,
    pub(super) Arc<tokio::sync::Mutex<()>>,
);
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct CachedModels {
    pub(super) models: Vec<ModelDef>,
    pub(super) refreshed_at: i64,
    #[serde(default)]
    pub(super) platform_id: String,
    #[serde(default)]
    pub(super) plugin_version: Value,
}
impl CachedModels {
    pub(super) fn supports_declared(&self, model: &ModelDef) -> bool {
        let required = model
            .extra
            .get("account_model")
            .and_then(Value::as_str)
            .or_else(|| {
                (model.task_kind == mediagen_core::TaskKind::Text)
                    .then_some(model.model_id.as_str())
            });
        required.is_none_or(|id| self.models.iter().any(|model| model.model_id == id))
    }
}

impl Catalog {
    pub(crate) fn load(path: &std::path::Path) -> Self {
        let cache = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self(Arc::new(Mutex::new(cache)), Default::default())
    }
}

impl Catalog {
    // 插件版本变化后丢弃旧目录，防止旧能力绕过新插件的型号和参数约束。
    pub(crate) fn retain_current_plugins(&self, loader: &PluginLoader) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, entry| {
                loader
                    .get_plugin(&entry.platform_id)
                    .is_some_and(|manifest| {
                        entry.plugin_version
                            == manifest
                                .extra
                                .get("version")
                                .cloned()
                                .unwrap_or(Value::Null)
                    })
            });
    }
}

/// 调度前补齐账号真实目录；没有目录不能把插件总目录当作账号权限。
pub(crate) async fn refresh_before_dispatch(state: &crate::ServerState, model: &str, key: &str) {
    let platform = state
        .plugin_loader
        .resolve_model(model)
        .map(|(manifest, _)| manifest.platform_id.clone())
        .or_else(|| super::resolve(state, model, key).map(|(manifest, _, _)| manifest.platform_id));
    let Some(platform) = platform else { return };
    let Some(manifest) = state.plugin_loader.get_plugin(&platform) else {
        return;
    };
    if !manifest.flows.contains_key("list_models") {
        return;
    }
    for account in super::visible_accounts(state, key)
        .into_iter()
        .filter(|account| {
            account.platform == platform
                && !matches!(account.login_state.as_str(), "disabled" | "login_expired")
        })
    {
        let stale = state
            .model_catalog
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&account.id)
            .is_none_or(|entry| chrono::Utc::now().timestamp() - entry.refreshed_at > 3600);
        if stale {
            let _ = super::refresh_account(state, account).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[actix_web::test]
    async fn dispatch_discovers_missing_catalog_and_excludes_accounts_without_model() {
        let state = crate::test_fixtures::protocol_state(true);
        let manifest = state.plugin_loader.get_plugin("test").unwrap();
        let mut other = crate::test_fixtures::account_record("plus", "test", "Plus", "");
        other.tier = "plus".into();
        state.store.insert_account(other);
        let scripts = state
            .plugin_loader
            .get_plugin_dir("test")
            .unwrap()
            .join("scripts");
        std::fs::write(scripts.join("models.js"), "function main(ctx){return {models:ctx.account_id==='account'?[{model_id:'text-model',display_name:'Pro',task_kind:'text'}]:[]}}").unwrap();
        assert!(super::super::account_model(&state, manifest, "text-model", "account").is_none());
        refresh_before_dispatch(&state, "text-model", "key-a").await;
        assert!(super::super::account_model(&state, manifest, "text-model", "account").is_some());
        assert!(super::super::account_model(&state, manifest, "text-model", "plus").is_none());
        let (account, _lease) = crate::protocol_task::reserve(
            &state,
            manifest,
            "text-model",
            "text",
            "key-a",
            "eligible",
            None,
        )
        .unwrap();
        assert_eq!(account.id, "account");
        // 有效期内不重复请求目录；目录刷新认证失败时不能继续显示可用。
        std::fs::write(
            scripts.join("models.js"),
            "function main(){throw new Error('LOGIN_EXPIRED: stale token')}",
        )
        .unwrap();
        refresh_before_dispatch(&state, "text-model", "key-a").await;
        assert_eq!(
            state.store.account_by_id("account").unwrap().login_state,
            "available"
        );
        let result =
            super::super::refresh_account(&state, state.store.account_by_id("account").unwrap())
                .await;
        assert!(result.is_err());
        assert_eq!(
            state.store.account_by_id("account").unwrap().login_state,
            "login_expired"
        );
    }

    #[test]
    fn startup_discards_old_and_unversioned_catalogs_but_keeps_current_accounts() {
        let root = std::env::temp_dir().join(format!("model-cache-{}", uuid::Uuid::new_v4()));
        let plugin = root.join("example");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("manifest.json"), json!({"platform_id":"example","display_name":"Example","version":"2.0","models":[],"flows":{}}).to_string()).unwrap();
        let loader = PluginLoader::load_from(&root).unwrap();
        let path = root.join("cache.json");
        std::fs::write(&path, json!({
            "old":{"models":[],"refreshed_at":1,"platform_id":"example","plugin_version":"1.0"},
            "current":{"models":[],"refreshed_at":1,"platform_id":"example","plugin_version":"2.0"},
            "legacy":{"models":[],"refreshed_at":1}
        }).to_string()).unwrap();
        let catalog = Catalog::load(&path);
        catalog.retain_current_plugins(&loader);
        let cache = catalog.0.lock().unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key("current"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[actix_web::test]
    async fn media_dispatch_filters_accounts_by_plugin_declared_model_dependency() {
        let source = crate::protocol_api_tests::state();
        let directory = source.plugin_loader.get_plugin_dir("test").unwrap();
        let path = directory.join("manifest.json");
        let mut manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        manifest["models"].as_array_mut().unwrap().push(json!({
            "model_id":"image-pro", "display_name":"Image Pro", "task_kind":"image", "account_model":"text-pro"
        }));
        std::fs::write(&path, manifest.to_string()).unwrap();
        let mut configured = source.get_ref().clone();
        configured.plugin_loader =
            Arc::new(PluginLoader::load_from(directory.parent().unwrap()).unwrap());
        let state = actix_web::web::Data::new(configured);
        let mut other = state.store.account_by_id("account").unwrap();
        other.id = "pro-account".into();
        state.store.insert_account(other);
        for (id, models) in [
            ("account", json!([])),
            (
                "pro-account",
                json!([{
                    "model_id":"text-pro", "display_name":"Pro", "task_kind":"text"
                }]),
            ),
        ] {
            state.model_catalog.0.lock().unwrap().insert(
                id.into(),
                CachedModels {
                    models: serde_json::from_value(models).unwrap(),
                    platform_id: "test".into(),
                    refreshed_at: chrono::Utc::now().timestamp(),
                    plugin_version: Value::Null,
                },
            );
        }
        let context = crate::media_execution::resolve_media_flow_context(
            &state,
            "image-pro",
            mediagen_core::TaskKind::Image,
            Some("key-a"),
            "image-filter".into(),
        )
        .unwrap();
        assert_eq!(context.account.id, "pro-account");
        assert_eq!(context.model.model_id, "image-pro");
        let unavailable = crate::media_execution::next_failover_context(
            &state,
            &context,
            Some("key-a"),
            &["pro-account".into()],
        )
        .unwrap();
        assert!(unavailable.is_none());
        let response = super::super::account_models(
            state.clone(),
            actix_web::web::Path::from("account".to_string()),
            actix_web::web::Query(super::super::Query {
                refresh: false,
                account_id: None,
            }),
        )
        .await;
        let body = actix_web::body::to_bytes(response.into_body())
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(!body["data"]["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "image-pro"));
        assert!(super::super::catalog(&state, "key-a")
            .iter()
            .any(|model| model["id"] == "image-pro"));
        state
            .model_catalog
            .0
            .lock()
            .unwrap()
            .get_mut("pro-account")
            .unwrap()
            .models
            .clear();
        assert!(!super::super::catalog(&state, "key-a")
            .iter()
            .any(|model| model["id"] == "image-pro"));
    }
}

pub(super) fn merge_capabilities(variants: &[Value]) -> Value {
    let mut merged = variants.first().cloned().unwrap_or(json!({}));
    for field in [
        "input_modalities",
        "output_modalities",
        "endpoints",
        "voices",
        "response_formats",
        "image_sources",
    ] {
        let mut values = Vec::new();
        for value in variants
            .iter()
            .flat_map(|variant| variant[field].as_array().into_iter().flatten())
        {
            if !values.contains(value) {
                values.push(value.clone());
            }
        }
        if !values.is_empty() {
            merged[field] = json!(values);
        }
    }
    for field in ["streaming", "tool_calling"] {
        if variants.iter().any(|variant| variant[field] == true) {
            merged[field] = json!(true);
        }
    }
    let unsupported = merged["unsupported_parameters"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|value| {
            variants.iter().all(|variant| {
                variant["unsupported_parameters"]
                    .as_array()
                    .is_some_and(|list| list.contains(value))
            })
        })
        .collect::<Vec<_>>();
    merged["unsupported_parameters"] = json!(unsupported);
    let mut efforts = Vec::new();
    for effort in variants.iter().flat_map(|variant| {
        variant["reasoning"]["efforts"]
            .as_array()
            .into_iter()
            .flatten()
    }) {
        if !efforts.contains(effort) {
            efforts.push(effort.clone());
        }
    }
    if !efforts.is_empty() {
        merged["reasoning"]["efforts"] = json!(efforts);
    }
    merged
}
