use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub platform_id: String,
    pub display_name: String,
    #[serde(default = "default_sort_order")]
    pub sort_order: i32,
    #[serde(default)]
    pub adapter_mode: AdapterMode,
    #[serde(default)]
    pub login_url: String,
    #[serde(default)]
    pub workspace_url: String,
    #[serde(default)]
    pub models: Vec<ModelDef>,
    #[serde(default)]
    pub tiers: HashMap<String, TierDef>,
    #[serde(default)]
    pub flows: HashMap<String, FlowDef>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDef {
    pub model_id: String,
    pub display_name: String,
    #[serde(default)]
    pub task_kind: TaskKind,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "default_alias_priority")]
    pub alias_priority: i32,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierDef {
    pub display_name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default = "default_limit_one")]
    pub concurrency_limit: i32,
    #[serde(default)]
    pub daily_limit: i32,
    #[serde(default)]
    pub total_limit: i32,
    #[serde(default)]
    pub cooldown_seconds: i32,
    #[serde(default)]
    pub task_interval_seconds: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDef {
    #[serde(default)]
    pub steps: Vec<serde_json::Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdapterMode {
    #[default]
    Browser,
    Api,
    Hybrid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    #[default]
    Image,
    Video,
}

fn default_sort_order() -> i32 {
    1000
}

fn default_alias_priority() -> i32 {
    1000
}

fn default_limit_one() -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn manifest_deserializes_defaults_and_extra_fields() {
        let manifest: PluginManifest = serde_json::from_value(json!({
            "platform_id": "platform-a",
            "display_name": "Platform A",
            "custom_flag": true,
            "models": [{ "model_id": "image", "display_name": "Image" }],
            "tiers": { "free": { "display_name": "Free" } },
            "flows": { "generate": { "steps": [{ "action": "noop" }] } }
        }))
        .unwrap();

        assert_eq!(manifest.platform_id, "platform-a");
        assert_eq!(manifest.sort_order, 1000);
        assert_eq!(manifest.adapter_mode, AdapterMode::Browser);
        assert_eq!(manifest.models[0].task_kind, TaskKind::Image);
        assert_eq!(manifest.models[0].alias_priority, 1000);
        assert_eq!(manifest.tiers["free"].concurrency_limit, 1);
        assert_eq!(manifest.flows["generate"].steps.len(), 1);
        assert_eq!(manifest.extra["custom_flag"], json!(true));
    }

    #[test]
    fn enum_values_use_snake_case_names() {
        let mode: AdapterMode = serde_json::from_str("\"hybrid\"").unwrap();
        let kind: TaskKind = serde_json::from_str("\"video\"").unwrap();

        assert_eq!(mode, AdapterMode::Hybrid);
        assert_eq!(kind, TaskKind::Video);
        assert_eq!(serde_json::to_string(&AdapterMode::Api).unwrap(), "\"api\"");
    }
}
