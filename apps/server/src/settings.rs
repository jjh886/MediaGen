use crate::access_control::normalize_setting_keys;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub api_port: u16,
    pub api_address: String,
    pub api_allowed_origins: String,
    pub api_keys: String,
    pub image_task_concurrency: u32,
    pub image_task_queue_limit: u32,
    pub video_task_concurrency: u32,
    pub video_task_queue_limit: u32,
    pub check_interval_minutes: u32,
    pub cooldown_default_seconds: u32,
    pub cache_dir: String,
    pub platform_tier_overrides: String,
    pub platform_generation_configs: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        // 这些默认值对应 Web Console 的表单字段，老数据缺字段时会自动补齐。
        Self {
            api_port: 18080,
            api_address: "0.0.0.0".to_string(),
            api_allowed_origins: "*".to_string(),
            api_keys: std::env::var("MEDIAGEN_API_KEYS").unwrap_or_default(),
            image_task_concurrency: 64,
            image_task_queue_limit: 512,
            video_task_concurrency: 8,
            video_task_queue_limit: 64,
            check_interval_minutes: 30,
            cooldown_default_seconds: 300,
            cache_dir: String::new(),
            platform_tier_overrides: "{}".to_string(),
            platform_generation_configs: "{}".to_string(),
        }
    }
}

impl AppSettings {
    pub fn platform_generation_config(&self, platform_id: &str) -> Value {
        // Rust 只按平台 ID 分发通用配置，字段含义和默认值仍由对应插件解释。
        serde_json::from_str::<Map<String, Value>>(&self.platform_generation_configs)
            .ok()
            .and_then(|configs| configs.get(platform_id).cloned())
            .filter(Value::is_object)
            .unwrap_or_else(|| Value::Object(Map::new()))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SettingsUpdate {
    pub api_port: Option<u16>,
    pub api_address: Option<String>,
    pub api_allowed_origins: Option<String>,
    pub api_keys: Option<String>,
    pub image_task_concurrency: Option<u32>,
    pub image_task_queue_limit: Option<u32>,
    pub video_task_concurrency: Option<u32>,
    pub video_task_queue_limit: Option<u32>,
    pub check_interval_minutes: Option<u32>,
    pub cooldown_default_seconds: Option<u32>,
    pub cache_dir: Option<String>,
    pub platform_tier_overrides: Option<String>,
    pub platform_generation_configs: Option<String>,
}

pub fn apply_settings_update(settings: &mut AppSettings, update: SettingsUpdate) {
    let SettingsUpdate {
        api_port,
        api_address,
        api_allowed_origins,
        api_keys,
        image_task_concurrency,
        image_task_queue_limit,
        video_task_concurrency,
        video_task_queue_limit,
        check_interval_minutes,
        cooldown_default_seconds,
        cache_dir,
        platform_tier_overrides,
        platform_generation_configs,
    } = update;

    set_if_some(&mut settings.api_port, api_port);
    set_trimmed(&mut settings.api_address, api_address);
    set_trimmed(&mut settings.api_allowed_origins, api_allowed_origins);
    set_api_keys(&mut settings.api_keys, api_keys);
    set_if_some(&mut settings.image_task_concurrency, image_task_concurrency);
    set_if_some(&mut settings.image_task_queue_limit, image_task_queue_limit);
    set_if_some(&mut settings.video_task_concurrency, video_task_concurrency);
    set_if_some(&mut settings.video_task_queue_limit, video_task_queue_limit);
    set_if_some(&mut settings.check_interval_minutes, check_interval_minutes);
    set_if_some(
        &mut settings.cooldown_default_seconds,
        cooldown_default_seconds,
    );
    set_trimmed(&mut settings.cache_dir, cache_dir);
    set_if_some(
        &mut settings.platform_tier_overrides,
        platform_tier_overrides,
    );
    set_json_object(
        &mut settings.platform_generation_configs,
        platform_generation_configs,
    );
}

fn set_if_some<T>(target: &mut T, value: Option<T>) {
    // None 表示前端没有提交这个字段，必须保留原配置。
    if let Some(value) = value {
        *target = value;
    }
}

fn set_trimmed(target: &mut String, value: Option<String>) {
    // 文本配置统一裁掉首尾空格，避免保存不可见字符。
    if let Some(value) = value {
        *target = value.trim().to_string();
    }
}

fn set_api_keys(target: &mut String, value: Option<String>) {
    // API Key 支持多行配置，需要走专门的规范化逻辑。
    if let Some(value) = value {
        *target = normalize_setting_keys(&value);
    }
}

fn set_json_object(target: &mut String, value: Option<String>) {
    let Some(value) = value else {
        return;
    };
    let normalized = value.trim();
    // 平台配置必须是 JSON 对象；无效输入不覆盖最后一次可用配置。
    if serde_json::from_str::<Map<String, Value>>(normalized).is_ok() {
        *target = normalized.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_settings_update, AppSettings, SettingsUpdate};

    #[test]
    fn legacy_port_ranges_are_ignored_without_losing_other_settings() {
        let settings: AppSettings = serde_json::from_value(serde_json::json!({
            "api_port": 19080,
            "api_end_port": 19089,
            "api_ports": "19080-19089",
            "api_port_count": 10,
            "image_task_concurrency": 12
        }))
        .unwrap();
        assert_eq!(settings.api_port, 19080);
        assert_eq!(settings.api_address, "0.0.0.0");
        assert_eq!(settings.image_task_concurrency, 12);
        let serialized = serde_json::to_value(settings).unwrap();
        for key in ["api_end_port", "api_ports", "api_port_count"] {
            assert!(serialized.get(key).is_none());
        }
    }

    #[actix_web::test]
    async fn settings_api_reports_runtime_binding_on_read_and_update() {
        use actix_web::test;
        use serde_json::{json, Value};

        let root = std::env::temp_dir().join(format!("settings-bind-{}", uuid::Uuid::new_v4()));
        let mut state = crate::test_fixtures::server_state(
            root.clone(),
            mediagen_plugin_runtime::PluginLoader::empty(),
        );
        state.api_bind = ("192.0.2.10".to_string(), 19080);
        state.store.update_settings(SettingsUpdate {
            api_address: Some("127.0.0.1".to_string()),
            api_port: Some(18081),
            ..SettingsUpdate::default()
        });
        let app = crate::init_admin_test_app!(state);
        let requests = [
            test::TestRequest::get().uri("/api/settings").to_request(),
            test::TestRequest::post()
                .uri("/api/settings")
                .set_json(json!({
                    "api_address": "127.0.0.1",
                    "api_port": 18082,
                    "api_end_port": 18089,
                    "api_ports": "18080-18089",
                    "api_port_count": 10,
                    "image_task_concurrency": 12
                }))
                .to_request(),
        ];
        for request in requests {
            let response = test::call_service(&app, request).await;
            assert!(response.status().is_success());
            let body: Value = test::read_body_json(response).await;
            assert_eq!(body["data"]["api_address"], "192.0.2.10");
            assert_eq!(body["data"]["api_port"], 19080);
            for key in ["api_end_port", "api_ports", "api_port_count"] {
                assert!(body["data"].get(key).is_none());
            }
        }
        let request = test::TestRequest::get().uri("/api/settings").to_request();
        let body: Value = test::call_and_read_body_json(&app, request).await;
        assert_eq!(body["data"]["image_task_concurrency"], 12);
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn platform_generation_config_is_selected_by_platform() {
        let settings = AppSettings {
            platform_generation_configs:
                r#"{"platform-a":{"profile":"balanced","nested":{"enabled":true}}}"#.to_string(),
            ..AppSettings::default()
        };

        let config = settings.platform_generation_config("platform-a");

        assert_eq!(config["profile"], "balanced");
        assert_eq!(config["nested"]["enabled"], true);
        assert!(settings
            .platform_generation_config("another-platform")
            .as_object()
            .is_some_and(|value| value.is_empty()));
    }

    #[test]
    fn invalid_platform_generation_config_does_not_replace_saved_value() {
        let mut settings = AppSettings {
            platform_generation_configs: r#"{"platform-a":{"profile":"balanced"}}"#.to_string(),
            ..AppSettings::default()
        };

        apply_settings_update(
            &mut settings,
            SettingsUpdate {
                platform_generation_configs: Some("not-json".to_string()),
                ..SettingsUpdate::default()
            },
        );

        assert_eq!(
            settings.platform_generation_configs,
            r#"{"platform-a":{"profile":"balanced"}}"#
        );
    }
}
