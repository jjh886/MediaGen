use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::types::{ModelDef, PluginManifest};

const OPENAI_IMAGE_MODEL_ALIASES: &[&str] = &[
    "gpt-image-2",
    "gpt-image-1.5",
    "gpt-image-1",
    "gpt-image-1-mini",
    "chatgpt-image-latest",
];

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

    /// 通过 OpenAI 请求里的 model 字段找到对应插件和模型定义。
    pub fn resolve_model(&self, model_id: &str) -> Option<(&PluginManifest, &ModelDef)> {
        if model_id.is_empty() {
            return None;
        }
        if let Some(resolved) = self.resolve_openai_alias(model_id) {
            return Some(resolved);
        }
        let prefix = model_id.split('_').next()?;
        if let Some(manifest) = self.plugins.get(prefix) {
            if let Some(model) = manifest.models.iter().find(|m| m.model_id == model_id) {
                return Some((manifest, model));
            }
        }
        // OpenAI 的 model 字段不要求带平台前缀；比如 ChatGPT 插件模型
        // `GPT_5_mini` 的前缀不是平台 ID。前缀命中失败时全局精确查找，
        // 保证 `/v1/models` 返回的任意 id 都能直接用于生成接口。
        self.sorted_plugins().into_iter().find_map(|manifest| {
            manifest
                .models
                .iter()
                .find(|model| model.model_id == model_id)
                .map(|model| (manifest, model))
        })
    }

    /// 兼容外部应用常见的 OpenAI 官方图片模型名。
    /// 用户仍可用 `/v1/models` 返回的真实插件模型名；这些别名只负责避免标准客户端硬编码模型时报 404。
    fn resolve_openai_alias(&self, model_id: &str) -> Option<(&PluginManifest, &ModelDef)> {
        if !OPENAI_IMAGE_MODEL_ALIASES.contains(&model_id) {
            return None;
        }
        self.resolve_first_existing(&[
            ("chatgpt", "GPT_5_mini"),
            ("GPTImage2", "GPTImage2_gpt_5_5"),
        ])
    }

    fn resolve_first_existing(
        &self,
        candidates: &[(&str, &str)],
    ) -> Option<(&PluginManifest, &ModelDef)> {
        for (platform_id, candidate_model_id) in candidates {
            let Some(manifest) = self.plugins.get(*platform_id) else {
                continue;
            };
            if let Some(model) = manifest
                .models
                .iter()
                .find(|m| m.model_id == *candidate_model_id)
            {
                return Some((manifest, model));
            }
        }
        None
    }

    /// 暴露给 `/v1/models` 的标准 OpenAI 图片模型别名。
    pub fn openai_image_aliases(&self) -> Vec<(&'static str, &PluginManifest, &ModelDef)> {
        OPENAI_IMAGE_MODEL_ALIASES
            .iter()
            .filter_map(|alias| {
                self.resolve_openai_alias(alias)
                    .map(|(manifest, model)| (*alias, manifest, model))
            })
            .collect()
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
        let manifest = minimal_manifest("chatgpt", vec![model_json("GPT_5_mini")]);
        std::fs::write(sub.join("manifest.json"), manifest).unwrap();

        let loader = PluginLoader::load_from(dir.path()).unwrap();

        let (plugin, model) = loader.resolve_model("GPT_5_mini").unwrap();
        assert_eq!(plugin.platform_id, "chatgpt");
        assert_eq!(model.model_id, "GPT_5_mini");
    }

    #[test]
    fn resolve_model_accepts_openai_image_aliases() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("chatgpt");
        std::fs::create_dir_all(&sub).unwrap();
        let manifest = minimal_manifest("chatgpt", vec![model_json("GPT_5_mini")]);
        std::fs::write(sub.join("manifest.json"), manifest).unwrap();

        let loader = PluginLoader::load_from(dir.path()).unwrap();

        let (plugin, model) = loader.resolve_model("gpt-image-1").unwrap();
        assert_eq!(plugin.platform_id, "chatgpt");
        assert_eq!(model.model_id, "GPT_5_mini");
    }

    #[test]
    fn openai_image_aliases_are_listable() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("chatgpt");
        std::fs::create_dir_all(&sub).unwrap();
        let manifest = minimal_manifest("chatgpt", vec![model_json("GPT_5_mini")]);
        std::fs::write(sub.join("manifest.json"), manifest).unwrap();

        let loader = PluginLoader::load_from(dir.path()).unwrap();
        let aliases: Vec<&str> = loader
            .openai_image_aliases()
            .into_iter()
            .map(|(alias, _, _)| alias)
            .collect();

        assert!(aliases.contains(&"gpt-image-1"));
        assert!(aliases.contains(&"chatgpt-image-latest"));
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
