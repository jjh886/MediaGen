use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mediagen_core::{ModelDef, PluginManifest};

pub struct PluginLoader {
    plugins: HashMap<String, PluginManifest>,
    plugin_dirs: HashMap<String, PathBuf>,
}

impl PluginLoader {
    pub fn empty() -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_dirs: HashMap::new(),
        }
    }

    pub fn load_from(plugins_dir: &Path) -> Result<Self, String> {
        let mut plugins = HashMap::new();
        let mut plugin_dirs = HashMap::new();
        let indexed_names = read_plugin_index(plugins_dir);

        for path in plugin_candidates(plugins_dir, indexed_names.as_deref())? {
            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }
            let content = std::fs::read_to_string(&manifest_path)
                .map_err(|e| format!("读取插件 manifest 失败 {}: {e}", manifest_path.display()))?;
            let manifest: PluginManifest = serde_json::from_str(&content)
                .map_err(|e| format!("解析插件 manifest 失败 {}: {e}", manifest_path.display()))?;
            plugin_dirs.insert(manifest.platform_id.clone(), path);
            plugins.insert(manifest.platform_id.clone(), manifest);
        }

        Ok(Self {
            plugins,
            plugin_dirs,
        })
    }

    pub fn plugins(&self) -> &HashMap<String, PluginManifest> {
        &self.plugins
    }

    pub fn get_plugin_dir(&self, platform_id: &str) -> Option<&Path> {
        self.plugin_dirs.get(platform_id).map(PathBuf::as_path)
    }

    pub fn get_plugin(&self, platform_id: &str) -> Option<&PluginManifest> {
        self.plugins.get(platform_id)
    }

    pub fn resolve_model(&self, model_id: &str) -> Option<(&PluginManifest, &ModelDef)> {
        for manifest in self.sorted_plugins() {
            if let Some(model) = manifest
                .models
                .iter()
                .find(|model| model.model_id == model_id)
            {
                return Some((manifest, model));
            }
        }
        self.model_aliases()
            .into_iter()
            .find(|(alias, _, _)| alias == model_id)
            .map(|(_, manifest, model)| (manifest, model))
    }

    pub fn sorted_plugins(&self) -> Vec<&PluginManifest> {
        let mut plugins: Vec<_> = self.plugins.values().collect();
        plugins.sort_by(|a, b| {
            a.sort_order
                .cmp(&b.sort_order)
                .then_with(|| a.platform_id.cmp(&b.platform_id))
        });
        plugins
    }

    pub fn all_models(&self) -> Vec<(&PluginManifest, &ModelDef)> {
        self.sorted_plugins()
            .into_iter()
            .flat_map(|manifest| manifest.models.iter().map(move |model| (manifest, model)))
            .collect()
    }

    pub fn model_aliases(&self) -> Vec<(String, &PluginManifest, &ModelDef)> {
        let mut seen = HashSet::new();
        let mut aliases = Vec::new();
        for manifest in self.sorted_plugins() {
            for model in &manifest.models {
                for alias in &model.aliases {
                    aliases.push((alias.clone(), manifest, model));
                }
            }
        }
        aliases.sort_by(|a, b| {
            a.2.alias_priority
                .cmp(&b.2.alias_priority)
                .then_with(|| a.1.sort_order.cmp(&b.1.sort_order))
                .then_with(|| a.1.platform_id.cmp(&b.1.platform_id))
                .then_with(|| a.2.model_id.cmp(&b.2.model_id))
        });
        aliases.retain(|(alias, _, _)| seen.insert(alias.clone()));
        aliases
    }
}

fn plugin_candidates(
    plugins_dir: &Path,
    indexed_names: Option<&[String]>,
) -> Result<Vec<PathBuf>, String> {
    if let Some(names) = indexed_names {
        return Ok(names.iter().map(|name| plugins_dir.join(name)).collect());
    }

    let entries = std::fs::read_dir(plugins_dir)
        .map_err(|e| format!("读取插件目录失败 {}: {e}", plugins_dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn read_plugin_index(plugins_dir: &Path) -> Option<Vec<String>> {
    let index_path = plugins_dir.join("plugins.index.json");
    let content = std::fs::read_to_string(index_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let plugins = value.get("plugins")?.as_array()?;
    Some(
        plugins
            .iter()
            .filter_map(|item| item.as_str())
            .filter(|name| is_safe_plugin_dir_name(name))
            .map(ToOwned::to_owned)
            .collect(),
    )
}

fn is_safe_plugin_dir_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use mediagen_core::{ModelDef, PluginManifest};

    use super::PluginLoader;

    #[test]
    fn duplicate_aliases_resolve_by_alias_priority() {
        let loader = PluginLoader {
            plugins: HashMap::from([
                (
                    "slow".to_string(),
                    manifest("slow", 10, model("slow-model", "shared", 50)),
                ),
                (
                    "fast".to_string(),
                    manifest("fast", 20, model("fast-model", "shared", 10)),
                ),
            ]),
            plugin_dirs: HashMap::new(),
        };

        let (manifest, model) = loader.resolve_model("shared").unwrap();

        assert_eq!(manifest.platform_id, "fast");
        assert_eq!(model.model_id, "fast-model");
        assert_eq!(loader.model_aliases().len(), 1);
    }

    #[test]
    fn load_from_uses_safe_plugin_index_entries() {
        let root = temp_plugins_dir("indexed");
        fs::create_dir_all(root.join("valid")).unwrap();
        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::write(
            root.join("plugins.index.json"),
            r#"{"plugins":["valid","../bad",""]}"#,
        )
        .unwrap();
        fs::write(root.join("valid/manifest.json"), manifest_json("valid", 5)).unwrap();
        fs::write(
            root.join("ignored/manifest.json"),
            manifest_json("ignored", 1),
        )
        .unwrap();

        let loader = PluginLoader::load_from(&root).unwrap();
        let valid_dir = root.join("valid");

        assert!(loader.get_plugin("valid").is_some());
        assert!(loader.get_plugin("ignored").is_none());
        assert_eq!(loader.get_plugin_dir("valid"), Some(valid_dir.as_path()));
        assert_eq!(
            loader.resolve_model("valid-model").unwrap().0.platform_id,
            "valid"
        );
    }

    #[test]
    fn load_from_without_index_discovers_plugin_dirs_sorted() {
        let root = temp_plugins_dir("discover");
        fs::create_dir_all(root.join("b")).unwrap();
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(root.join("b/manifest.json"), manifest_json("b", 20)).unwrap();
        fs::write(root.join("a/manifest.json"), manifest_json("a", 10)).unwrap();

        let loader = PluginLoader::load_from(&root).unwrap();
        let sorted = loader
            .sorted_plugins()
            .into_iter()
            .map(|item| item.platform_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(sorted, vec!["a", "b"]);
        assert_eq!(loader.all_models().len(), 2);
    }

    fn manifest(platform_id: &str, sort_order: i32, model: ModelDef) -> PluginManifest {
        PluginManifest {
            platform_id: platform_id.to_string(),
            display_name: platform_id.to_string(),
            sort_order,
            login_url: String::new(),
            workspace_url: String::new(),
            models: vec![model],
            tiers: HashMap::new(),
            flows: HashMap::new(),
            adapter_mode: Default::default(),
            extra: HashMap::new(),
        }
    }

    fn temp_plugins_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mediagen-plugin-loader-{label}-{nanos}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn manifest_json(platform_id: &str, sort_order: i32) -> String {
        format!(
            r#"{{
                "platform_id": "{platform_id}",
                "display_name": "{platform_id}",
                "sort_order": {sort_order},
                "models": [
                    {{
                        "model_id": "{platform_id}-model",
                        "display_name": "{platform_id} model",
                        "aliases": ["{platform_id}-alias"]
                    }}
                ]
            }}"#
        )
    }

    fn model(model_id: &str, alias: &str, alias_priority: i32) -> ModelDef {
        ModelDef {
            model_id: model_id.to_string(),
            display_name: model_id.to_string(),
            task_kind: Default::default(),
            aliases: vec![alias.to_string()],
            alias_priority,
            extra: HashMap::new(),
        }
    }
}
