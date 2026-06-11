use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::types::{ModelDef, PluginManifest};

pub struct PluginLoader {
    plugins: HashMap<String, PluginManifest>,
    plugin_dirs: HashMap<String, PathBuf>,
}

impl PluginLoader {
    /// 创建一个空的 PluginLoader，不包含任何插件。
    pub fn empty() -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_dirs: HashMap::new(),
        }
    }

    /// 扫描 plugins_dir 下的子目录，读取每个子目录中的 manifest.json。
    /// manifest 解析失败时跳过该插件并记录警告，不影响其他插件加载。
    pub fn load_from(plugins_dir: &Path) -> Result<Self, AppError> {
        let mut plugins = HashMap::new();
        let mut plugin_dirs = HashMap::new();

        let entries = std::fs::read_dir(plugins_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }

            let content = match std::fs::read_to_string(&manifest_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("读取 manifest 失败 {}: {}", manifest_path.display(), e);
                    continue;
                }
            };

            let manifest: PluginManifest = match serde_json::from_str(&content) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("解析 manifest 失败 {}: {}", manifest_path.display(), e);
                    continue;
                }
            };

            let platform_id = manifest.platform_id.clone();
            plugin_dirs.insert(platform_id.clone(), path);
            plugins.insert(platform_id, manifest);
        }

        Ok(Self {
            plugins,
            plugin_dirs,
        })
    }

    pub fn plugins(&self) -> &HashMap<String, PluginManifest> {
        &self.plugins
    }

    pub fn sorted_plugins(&self) -> Vec<&PluginManifest> {
        let mut plugins: Vec<&PluginManifest> = self.plugins.values().collect();
        plugins.sort_by(|a, b| {
            a.sort_order
                .cmp(&b.sort_order)
                .then_with(|| a.platform_id.cmp(&b.platform_id))
        });
        plugins
    }

    pub fn get_plugin(&self, platform_id: &str) -> Option<&PluginManifest> {
        self.plugins.get(platform_id)
    }

    pub fn get_plugin_dir(&self, platform_id: &str) -> Option<&Path> {
        self.plugin_dirs.get(platform_id).map(|p| p.as_path())
    }

    /// 通过请求里的 model 字段找到对应插件和模型定义。
    pub fn resolve_model(&self, model_id: &str) -> Option<(&PluginManifest, &ModelDef)> {
        if model_id.is_empty() {
            return None;
        }
        let prefix = model_id.split('_').next()?;
        if let Some(manifest) = self.plugins.get(prefix) {
            if let Some(model) = manifest.models.iter().find(|m| m.model_id == model_id) {
                return Some((manifest, model));
            }
        }
        if let Some(resolved) = self.resolve_exact_model(model_id) {
            return Some(resolved);
        }
        self.resolve_model_alias(model_id)
    }

    fn resolve_exact_model(&self, model_id: &str) -> Option<(&PluginManifest, &ModelDef)> {
        self.sorted_plugins().into_iter().find_map(|manifest| {
            manifest
                .models
                .iter()
                .find(|model| model.model_id == model_id)
                .map(|model| (manifest, model))
        })
    }

    fn resolve_model_alias(&self, alias: &str) -> Option<(&PluginManifest, &ModelDef)> {
        self.model_alias_entries(Some(alias))
            .into_iter()
            .next()
            .map(|(_, manifest, model)| (manifest, model))
    }

    /// 暴露给 `/v1/models` 的模型别名。别名来自插件 manifest，Rust 不写死平台偏好。
    pub fn model_aliases(&self) -> Vec<(String, &PluginManifest, &ModelDef)> {
        let mut seen = HashSet::new();
        let mut aliases = Vec::new();
        for (alias, manifest, model) in self.model_alias_entries(None) {
            if seen.insert(alias.clone()) {
                aliases.push((alias, manifest, model));
            }
        }
        aliases
    }

    fn model_alias_entries(
        &self,
        alias_filter: Option<&str>,
    ) -> Vec<(String, &PluginManifest, &ModelDef)> {
        let mut entries = Vec::new();
        for manifest in self.sorted_plugins() {
            for model in &manifest.models {
                for alias in &model.aliases {
                    if alias_filter.is_some_and(|filter| filter != alias) {
                        continue;
                    }
                    entries.push((alias.clone(), manifest, model));
                }
            }
        }
        entries.sort_by(|a, b| {
            a.2.alias_priority
                .cmp(&b.2.alias_priority)
                .then_with(|| a.1.sort_order.cmp(&b.1.sort_order))
                .then_with(|| a.1.platform_id.cmp(&b.1.platform_id))
                .then_with(|| a.2.model_id.cmp(&b.2.model_id))
        });
        entries
    }

    pub fn all_models(&self) -> Vec<(&PluginManifest, &ModelDef)> {
        self.sorted_plugins()
            .into_iter()
            .flat_map(|manifest| manifest.models.iter().map(move |model| (manifest, model)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn minimal_manifest(platform_id: &str, models: Vec<serde_json::Value>) -> String {
        serde_json::json!({
            "platform_id": platform_id,
            "display_name": platform_id,
            "adapter_mode": "browser",
            "login_url": "https://example.com/login",
            "workspace_url": "https://example.com",
            "models": models,
            "tiers": {},
            "flows": {}
        })
        .to_string()
    }

    fn minimal_manifest_with_sort_order(
        platform_id: &str,
        sort_order: i32,
        models: Vec<serde_json::Value>,
    ) -> String {
        serde_json::json!({
            "platform_id": platform_id,
            "display_name": platform_id,
            "sort_order": sort_order,
            "adapter_mode": "browser",
            "login_url": "https://example.com/login",
            "workspace_url": "https://example.com",
            "models": models,
            "tiers": {},
            "flows": {}
        })
        .to_string()
    }

    fn model_json(model_id: &str) -> serde_json::Value {
        serde_json::json!({
            "model_id": model_id,
            "display_name": model_id,
            "task_kind": "image"
        })
    }

    #[test]
    fn test_load_plugins() {
        let dir = tempdir().unwrap();
        for name in ["doubao", "flow"] {
            let sub = dir.path().join(name);
            std::fs::create_dir_all(&sub).unwrap();
            let manifest = minimal_manifest(name, vec![model_json(&format!("{name}_v1"))]);
            std::fs::write(sub.join("manifest.json"), manifest).unwrap();
        }

        let loader = PluginLoader::load_from(dir.path()).unwrap();
        assert_eq!(loader.plugins().len(), 2);
        assert!(loader.get_plugin("doubao").is_some());
        assert!(loader.get_plugin("flow").is_some());
        assert!(loader.get_plugin_dir("doubao").is_some());
    }

    #[test]
    fn test_sorted_plugins_uses_manifest_sort_order() {
        let dir = tempdir().unwrap();
        for (name, sort_order) in [("flow", 20), ("doubao", 10), ("chatgpt", 10)] {
            let sub = dir.path().join(name);
            std::fs::create_dir_all(&sub).unwrap();
            let manifest = minimal_manifest_with_sort_order(
                name,
                sort_order,
                vec![model_json(&format!("{name}_v1"))],
            );
            std::fs::write(sub.join("manifest.json"), manifest).unwrap();
        }

        let loader = PluginLoader::load_from(dir.path()).unwrap();
        let ids: Vec<&str> = loader
            .sorted_plugins()
            .into_iter()
            .map(|m| m.platform_id.as_str())
            .collect();

        assert_eq!(ids, vec!["chatgpt", "doubao", "flow"]);
    }

    #[test]
    fn test_resolve_model() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("doubao");
        std::fs::create_dir_all(&sub).unwrap();
        let manifest = minimal_manifest(
            "doubao",
            vec![
                model_json("doubao_Image_5_0_Lite"),
                model_json("doubao_Video_1"),
            ],
        );
        std::fs::write(sub.join("manifest.json"), manifest).unwrap();

        let loader = PluginLoader::load_from(dir.path()).unwrap();

        let (plugin, model) = loader.resolve_model("doubao_Image_5_0_Lite").unwrap();
        assert_eq!(plugin.platform_id, "doubao");
        assert_eq!(model.model_id, "doubao_Image_5_0_Lite");

        assert!(loader.resolve_model("unknown_model").is_none());
    }

    #[test]
    fn resolve_model_falls_back_to_exact_id_without_platform_prefix() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("chatgpt");
        std::fs::create_dir_all(&sub).unwrap();
        let manifest = minimal_manifest("chatgpt", vec![model_json("ChatGPT")]);
        std::fs::write(sub.join("manifest.json"), manifest).unwrap();

        let loader = PluginLoader::load_from(dir.path()).unwrap();

        let (plugin, model) = loader.resolve_model("ChatGPT").unwrap();
        assert_eq!(plugin.platform_id, "chatgpt");
        assert_eq!(model.model_id, "ChatGPT");
    }

    #[test]
    fn resolve_model_accepts_manifest_aliases() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("generic");
        std::fs::create_dir_all(&sub).unwrap();
        let manifest = minimal_manifest(
            "generic",
            vec![serde_json::json!({
                "model_id": "generic_image",
                "display_name": "Generic Image",
                "task_kind": "image",
                "aliases": ["gpt-image-1"],
                "alias_priority": 10
            })],
        );
        std::fs::write(sub.join("manifest.json"), manifest).unwrap();

        let loader = PluginLoader::load_from(dir.path()).unwrap();

        let (plugin, model) = loader.resolve_model("gpt-image-1").unwrap();
        assert_eq!(plugin.platform_id, "generic");
        assert_eq!(model.model_id, "generic_image");
    }

    #[test]
    fn duplicate_aliases_use_priority_then_sort_order() {
        let dir = tempdir().unwrap();
        for (name, priority) in [("slow", 20), ("fast", 10)] {
            let sub = dir.path().join(name);
            std::fs::create_dir_all(&sub).unwrap();
            let manifest = minimal_manifest(
                name,
                vec![serde_json::json!({
                    "model_id": format!("{name}_image"),
                    "display_name": name,
                    "task_kind": "image",
                    "aliases": ["gpt-image-1"],
                    "alias_priority": priority
                })],
            );
            std::fs::write(sub.join("manifest.json"), manifest).unwrap();
        }

        let loader = PluginLoader::load_from(dir.path()).unwrap();

        let (plugin, model) = loader.resolve_model("gpt-image-1").unwrap();
        assert_eq!(plugin.platform_id, "fast");
        assert_eq!(model.model_id, "fast_image");
        assert_eq!(loader.model_aliases().len(), 1);
    }

    #[test]
    fn test_resolve_model_empty_input_returns_none() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("doubao");
        std::fs::create_dir_all(&sub).unwrap();
        let manifest = minimal_manifest(
            "doubao",
            vec![serde_json::json!({
                "model_id": "doubao_text2img",
                "display_name": "豆包文生图",
                "task_kind": "image"
            })],
        );
        std::fs::write(sub.join("manifest.json"), manifest).unwrap();

        let loader = PluginLoader::load_from(dir.path()).unwrap();
        assert!(loader.resolve_model("").is_none());
    }

    #[test]
    fn test_all_models() {
        let dir = tempdir().unwrap();
        for (name, count) in [("doubao", 2), ("flow", 1)] {
            let sub = dir.path().join(name);
            std::fs::create_dir_all(&sub).unwrap();
            let models: Vec<_> = (0..count)
                .map(|i| model_json(&format!("{name}_m{i}")))
                .collect();
            let manifest = minimal_manifest(name, models);
            std::fs::write(sub.join("manifest.json"), manifest).unwrap();
        }

        let loader = PluginLoader::load_from(dir.path()).unwrap();
        assert_eq!(loader.all_models().len(), 3);
    }

    /// Flow manifest 必须解析为 Persistent；豆包 manifest（不声明字段）必须保持 PerTask。
    /// 直接使用项目内真实 manifest，确保改造后平台语义正确。
    #[test]
    fn browser_lifecycle_matches_shipped_manifests() {
        use crate::types::BrowserLifecycle;

        let plugins_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri parent")
            .join("plugins");

        let loader = PluginLoader::load_from(&plugins_root).expect("加载项目内 plugins 目录");

        let flow = loader.get_plugin("flow").expect("flow 插件应存在");
        assert_eq!(flow.browser_lifecycle, BrowserLifecycle::Persistent);

        let doubao = loader.get_plugin("doubao").expect("doubao 插件应存在");
        assert_eq!(doubao.browser_lifecycle, BrowserLifecycle::PerTask);
    }

    #[test]
    fn shipped_manifests_include_jimeng() {
        let plugins_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri parent")
            .join("plugins");

        let loader = PluginLoader::load_from(&plugins_root).expect("加载项目内 plugins 目录");
        let jimeng = loader.get_plugin("jimeng").expect("即梦插件应存在");

        assert_eq!(jimeng.display_name, "即梦");
        assert_eq!(jimeng.models.len(), 5);
    }

    #[test]
    fn test_skip_malformed() {
        let dir = tempdir().unwrap();

        // 正常插件
        let good = dir.path().join("good");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::write(
            good.join("manifest.json"),
            minimal_manifest("good", vec![model_json("good_v1")]),
        )
        .unwrap();

        // 损坏的 manifest
        let bad = dir.path().join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("manifest.json"), "{ invalid json !!!").unwrap();

        let loader = PluginLoader::load_from(dir.path()).unwrap();
        assert_eq!(loader.plugins().len(), 1);
        assert!(loader.get_plugin("good").is_some());
    }
}
