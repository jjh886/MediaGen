use std::path::{Path, PathBuf};

use crate::error::AppError;

const PLUGIN_INDEX_FILE: &str = "plugins.index.json";

pub(super) struct PluginCandidate {
    pub(super) expected_platform_id: Option<String>,
    pub(super) path: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
struct PluginIndex {
    #[serde(default)]
    plugins: Vec<String>,
}

pub(super) fn plugin_candidates(
    plugins_dir: &Path,
    indexed_names: Option<&[String]>,
) -> Result<Vec<PluginCandidate>, AppError> {
    if let Some(names) = indexed_names {
        return Ok(names
            .iter()
            .map(|name| PluginCandidate {
                expected_platform_id: Some(name.clone()),
                path: plugins_dir.join(name),
            })
            .collect());
    }

    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(plugins_dir)? {
        let path = entry?.path();
        if path.is_dir() {
            candidates.push(PluginCandidate {
                expected_platform_id: None,
                path,
            });
        }
    }
    Ok(candidates)
}

pub(super) fn read_plugin_index(plugins_dir: &Path) -> Option<Vec<String>> {
    let index_path = plugins_dir.join(PLUGIN_INDEX_FILE);
    if !index_path.exists() {
        return None;
    }
    let content = match std::fs::read_to_string(&index_path) {
        Ok(content) => content,
        Err(e) => {
            tracing::warn!("读取插件索引失败 {}: {}", index_path.display(), e);
            return Some(Vec::new());
        }
    };
    let index = match serde_json::from_str::<PluginIndex>(&content) {
        Ok(index) => index,
        Err(e) => {
            tracing::warn!("解析插件索引失败 {}: {}", index_path.display(), e);
            return Some(Vec::new());
        }
    };

    let mut names = Vec::new();
    for name in index.plugins {
        if !is_safe_plugin_dir_name(&name) {
            tracing::warn!(name, "插件索引包含非法目录名，已跳过");
            continue;
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Some(names)
}

fn is_safe_plugin_dir_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
