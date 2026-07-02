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
            for model in &manifest.models {
                if model.model_id == model_id || model.aliases.iter().any(|alias| alias == model_id)
                {
                    return Some((manifest, model));
                }
            }
        }
        None
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
                    if seen.insert(alias.clone()) {
                        aliases.push((alias.clone(), manifest, model));
                    }
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
