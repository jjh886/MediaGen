use crate::access_control::normalize_setting_keys;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub api_port: u16,
    pub api_end_port: u16,
    pub api_ports: String,
    pub api_port_count: u16,
    pub api_address: String,
    pub api_allowed_origins: String,
    pub api_keys: String,
    pub image_task_concurrency: u32,
    pub image_task_queue_limit: u32,
    pub flow_mirror_url: String,
    pub check_interval_minutes: u32,
    pub cooldown_default_seconds: u32,
    pub cache_dir: String,
    pub platform_tier_overrides: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        // 这些默认值对应 Web Console 的表单字段，老数据缺字段时会自动补齐。
        Self {
            api_port: 18080,
            api_end_port: 18089,
            api_ports: "18080".to_string(),
            api_port_count: 10,
            api_address: "127.0.0.1".to_string(),
            api_allowed_origins: "*".to_string(),
            api_keys: std::env::var("MEDIAGEN_API_KEYS").unwrap_or_default(),
            image_task_concurrency: 64,
            image_task_queue_limit: 512,
            flow_mirror_url: String::new(),
            check_interval_minutes: 30,
            cooldown_default_seconds: 300,
            cache_dir: String::new(),
            platform_tier_overrides: "{}".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SettingsUpdate {
    pub api_port: Option<u16>,
    pub api_end_port: Option<u16>,
    pub api_ports: Option<String>,
    pub api_port_count: Option<u16>,
    pub api_address: Option<String>,
    pub api_allowed_origins: Option<String>,
    pub api_keys: Option<String>,
    pub image_task_concurrency: Option<u32>,
    pub image_task_queue_limit: Option<u32>,
    pub flow_mirror_url: Option<String>,
    pub check_interval_minutes: Option<u32>,
    pub cooldown_default_seconds: Option<u32>,
    pub cache_dir: Option<String>,
    pub platform_tier_overrides: Option<String>,
}

pub fn apply_settings_update(settings: &mut AppSettings, update: SettingsUpdate) {
    let SettingsUpdate {
        api_port,
        api_end_port,
        api_ports,
        api_port_count,
        api_address,
        api_allowed_origins,
        api_keys,
        image_task_concurrency,
        image_task_queue_limit,
        flow_mirror_url,
        check_interval_minutes,
        cooldown_default_seconds,
        cache_dir,
        platform_tier_overrides,
    } = update;

    set_if_some(&mut settings.api_port, api_port);
    set_if_some(&mut settings.api_end_port, api_end_port);
    set_trimmed(&mut settings.api_ports, api_ports);
    set_if_some(&mut settings.api_port_count, api_port_count);
    set_trimmed(&mut settings.api_address, api_address);
    set_trimmed(&mut settings.api_allowed_origins, api_allowed_origins);
    set_api_keys(&mut settings.api_keys, api_keys);
    set_if_some(&mut settings.image_task_concurrency, image_task_concurrency);
    set_if_some(&mut settings.image_task_queue_limit, image_task_queue_limit);
    set_trimmed(&mut settings.flow_mirror_url, flow_mirror_url);
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
