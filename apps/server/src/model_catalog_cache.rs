use mediagen_core::ModelDef;
use mediagen_plugin_runtime::PluginLoader;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
