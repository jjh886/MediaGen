use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── 账号 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformAccount {
    pub id: String,
    pub platform: String,
    pub name: String,
    pub notes: String,
    pub login_state: LoginState,
    pub login_state_reason: String,
    pub login_state_updated_at: DateTime<Utc>,
    pub cookies: String,
    pub api_token: String,
    pub refresh_token: String,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub profile_dir: String,
    pub tier: String,
    pub concurrency_limit: i32,
    pub daily_limit: i32,
    pub total_limit: i32,
    pub used_today: i32,
    pub used_total: i32,
    pub usage_date: String,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub avatar_url: String,
    pub password_encrypted: String,
    pub created_at: DateTime<Utc>,
    pub last_check_at: Option<DateTime<Utc>>,
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    Available,
    DailyQuotaExhausted,
    TotalQuotaExhausted,
    LoginExpired,
    VerificationRequired,
    Cooldown,
    Disabled,
}

#[cfg(test)]
pub fn platform_account_fixture(id: &str, platform: &str) -> PlatformAccount {
    let now = Utc::now();
    let timestamp = now.to_rfc3339();
    // 测试只需要一个合法账号基线；用 JSON 样板避免在多个测试模块复制字段清单。
    serde_json::from_value(serde_json::json!({
        "id": id,
        "platform": platform,
        "name": "",
        "notes": "",
        "login_state": "available",
        "login_state_reason": "",
        "login_state_updated_at": timestamp,
        "cookies": "",
        "api_token": "",
        "refresh_token": "",
        "token_expires_at": null,
        "profile_dir": "",
        "tier": "free",
        "concurrency_limit": 1,
        "daily_limit": 0,
        "total_limit": 0,
        "used_today": 0,
        "used_total": 0,
        "usage_date": "",
        "cooldown_until": null,
        "user_id": "",
        "username": "",
        "email": "",
        "avatar_url": "",
        "password_encrypted": "",
        "created_at": timestamp,
        "last_check_at": timestamp,
        "last_refreshed_at": null,
        "last_used_at": null,
    }))
    .expect("测试账号样板必须能反序列化")
}

// ── 任务 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub platform: String,
    pub task_kind: TaskKind,
    pub model_id: String,
    pub prompt: String,
    pub account_id: Option<String>,
    pub status: TaskStatus,
    pub result_url: Option<String>,
    pub storage_key: Option<String>,
    pub error_message: Option<String>,
    /// 完整响应数据（JSON 格式，包含 url/width/height/source 等）
    pub result_data: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Image,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

// ── 插件 Manifest ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub platform_id: String,
    pub display_name: String,
    #[serde(default = "default_plugin_sort_order")]
    pub sort_order: i32,
    pub adapter_mode: AdapterMode,
    pub login_url: String,
    pub workspace_url: String,
    pub models: Vec<ModelDef>,
    pub tiers: HashMap<String, TierDef>,
    #[serde(default)]
    pub watermark: Option<WatermarkConfig>,
    pub flows: HashMap<String, FlowDef>,
    #[serde(default)]
    pub registration: Option<RegistrationConfig>,
    /// 登录检测配置（浏览器模式下使用）
    #[serde(default)]
    pub login_detection: Option<LoginDetection>,
    /// 后台 HTTP 登录状态检查（不启动浏览器，定期用 cookies 验证）
    #[serde(default)]
    pub login_check: Option<LoginCheck>,
    /// 登录成功后提取用户信息的脚本配置
    #[serde(default)]
    pub userinfo_script: Option<ScriptRef>,
    /// 当触发 rate_limited 且需要人工验证兜底时，指定用另一个插件的 DOM flow 重跑。
    /// 用于 API 模式插件：API 直调被限流时不会弹验证码，需借助 DOM flow
    /// 的交互过程让平台风控系统插入验证码 iframe，让人工过验证。
    #[serde(default)]
    pub verification_fallback: Option<VerificationFallback>,
    /// 浏览器生命周期；不填或未识别值都视作 `PerTask`，保持现有平台行为不变。
    #[serde(default)]
    pub browser_lifecycle: BrowserLifecycle,
}

fn default_plugin_sort_order() -> i32 {
    1000
}

/// 跨插件的人工验证兜底配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationFallback {
    /// 兜底使用的平台 id（如 "doubao"）
    pub platform: String,
    /// 兜底使用的 flow 名字，默认 "generate_image"
    #[serde(default = "default_verification_flow_name")]
    pub flow: String,
}

fn default_verification_flow_name() -> String {
    "generate_image".to_string()
}

/// 登录检测方式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum LoginDetection {
    /// 通过 CDP 协议检查 httpOnly cookies（如 sessionid）
    CdpCookies {
        /// 任一 cookie 存在即视为已登录
        cookie_names: Vec<String>,
    },
    /// 通过执行 JS 脚本检测（脚本返回 JSON 含 loggedIn 字段）
    JsScript {
        script_file: String,
        #[serde(default)]
        await_promise: bool,
        /// 运行时填充的脚本内容（从 scripts/ 目录读取）
        #[serde(skip)]
        script_content: Option<String>,
    },
}

/// 后台 HTTP 登录状态检查配置（不启动浏览器，用存储的 cookies 发请求验证）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginCheck {
    /// 验证接口 URL
    pub url: String,
    /// cookies 域名过滤关键词（匹配 domain 中包含此字符串的 cookie）
    pub cookie_domain: String,
    /// 响应 JSON 中判断已登录的字段路径（如 "user.email"、"data.user_id_str"）
    pub success_field: String,
    /// 可选：响应 JSON 中的过期时间字段（ISO 8601 格式），值在当前时间之前则视为过期
    #[serde(default)]
    pub expires_field: Option<String>,
}

/// 指向插件 scripts/ 目录下的脚本文件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRef {
    pub script_file: String,
    #[serde(default)]
    pub await_promise: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AdapterMode {
    Api,
    Browser,
}

/// 浏览器生命周期：控制任务结束后是否保留 Chrome 进程。
///
/// - `PerTask`（默认）：每次任务 open → close 关闭 Chrome。豆包等国内平台适用。
/// - `Persistent`：登录后常驻，仅在 relogin / 删除账号 / App 退出时关闭。
///   Flow 这类 Google 强反爬平台需要避免"每次 Chrome 重启 + cookie 注入被识别为 session 搬家"。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserLifecycle {
    #[default]
    PerTask,
    Persistent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDef {
    pub model_id: String,
    pub display_name: String,
    pub task_kind: TaskKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierDef {
    pub display_name: String,
    #[serde(default)]
    pub is_default: bool,
    pub concurrency_limit: i32,
    pub daily_limit: i32,
    pub total_limit: i32,
    pub cooldown_seconds: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatermarkConfig {
    pub has_watermark: bool,
    pub removal_strategy: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDef {
    pub steps: Vec<StepDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDef {
    pub action: String,
    #[serde(flatten)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationConfig {
    pub enabled: bool,
    #[serde(default)]
    pub steps: Vec<StepDef>,
}

// ── 流程执行结果 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub success: bool,
    pub data: serde_json::Value,
    pub error: Option<String>,
}

// ── 日志 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    pub id: String,
    pub timestamp: String,
    pub module: String,
    pub level: String,
    pub summary: String,
    pub detail: Option<String>,
    pub task_id: Option<String>,
}

// ── 调度结果 ──

#[derive(Debug, Clone, PartialEq)]
pub enum AllocationResult {
    /// 成功分配到账号
    Allocated(String),
    /// 没有该平台的账号
    NoAccounts,
    /// 所有账号都在忙（并发满/冷却中/登录过期），可稍后重试
    AllBusy,
    /// 所有账号配额耗尽，需要注册新账号
    AllQuotaExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不填 `browser_lifecycle` 字段时必须落到 PerTask，保证老 manifest 行为不变。
    #[test]
    fn browser_lifecycle_defaults_to_per_task() {
        assert_eq!(BrowserLifecycle::default(), BrowserLifecycle::PerTask);
    }

    /// 字符串反序列化覆盖 `per_task` / `persistent` 两个取值。
    #[test]
    fn browser_lifecycle_serde_snake_case() {
        let per_task: BrowserLifecycle =
            serde_json::from_value(serde_json::json!("per_task")).unwrap();
        assert_eq!(per_task, BrowserLifecycle::PerTask);

        let persistent: BrowserLifecycle =
            serde_json::from_value(serde_json::json!("persistent")).unwrap();
        assert_eq!(persistent, BrowserLifecycle::Persistent);
    }
}
