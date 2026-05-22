use std::sync::Arc;
use std::sync::Mutex;

use chrono::{Duration, Utc};

use crate::error::AppError;
use crate::plugin_loader::PluginLoader;
use crate::storage::Storage;
use crate::sync_ext::MutexExt;
use crate::types::{LoginState, PlatformAccount, StepResult};

pub struct AccountManager {
    storage: Arc<Mutex<Storage>>,
}

impl AccountManager {
    pub fn new(storage: Arc<Mutex<Storage>>) -> Self {
        Self { storage }
    }

    pub async fn add_account(&self, account: PlatformAccount) -> Result<(), AppError> {
        let db = self.storage.lock_recover();
        db.insert_account(&account)
    }

    pub async fn remove_account(&self, id: &str) -> Result<(), AppError> {
        let db = self.storage.lock_recover();
        db.delete_account(id)
    }

    pub async fn list_accounts(&self, platform: &str) -> Result<Vec<PlatformAccount>, AppError> {
        let db = self.storage.lock_recover();
        db.list_accounts_by_platform(platform)
    }

    pub async fn get_account(&self, id: &str) -> Result<Option<PlatformAccount>, AppError> {
        let db = self.storage.lock_recover();
        db.get_account(id)
    }

    pub async fn update_state(
        &self,
        id: &str,
        state: LoginState,
        reason: &str,
    ) -> Result<(), AppError> {
        let db = self.storage.lock_recover();
        db.update_login_state(id, state, reason)
    }

    pub async fn update_account(&self, account: &PlatformAccount) -> Result<(), AppError> {
        let db = self.storage.lock_recover();
        db.update_account(account)
    }

    pub async fn increment_usage(&self, id: &str) -> Result<(), AppError> {
        let db = self.storage.lock_recover();
        let mut account = db
            .get_account(id)?
            .ok_or_else(|| AppError::AccountUnavailable(format!("账号不存在: {id}")))?;

        account.used_today += 1;
        account.used_total += 1;

        if account.total_limit > 0 && account.used_total >= account.total_limit {
            db.update_login_state(id, LoginState::TotalQuotaExhausted, "总用量已达上限")?;
        } else if account.daily_limit > 0 && account.used_today >= account.daily_limit {
            db.update_login_state(id, LoginState::DailyQuotaExhausted, "当日用量已达上限")?;
        }

        db.update_usage(id, account.used_today, account.used_total)
    }

    pub async fn reset_daily_quotas(&self) -> Result<u64, AppError> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let db = self.storage.lock_recover();
        let count = db.reset_daily_usage(&today)?;
        // 将 DailyQuotaExhausted 状态的账号恢复为 Available
        let platforms = db.list_distinct_platforms()?;
        for platform in platforms {
            let accounts = db.list_accounts_by_platform(&platform)?;
            for acc in accounts {
                if acc.login_state == LoginState::DailyQuotaExhausted {
                    db.update_login_state(&acc.id, LoginState::Available, "每日配额已重置")?;
                }
            }
        }
        Ok(count)
    }

    pub async fn get_all_platforms(&self) -> Result<Vec<String>, AppError> {
        let db = self.storage.lock_recover();
        db.list_distinct_platforms()
    }
}

/// 错误关键词 → 账号状态映射表（cooldown_secs: 0=无冷却, -1=使用 tier 默认值）
/// 返回 None 表示不需要更新账号状态（如内容违规）
const FAILURE_RULES: &[(&[&str], Option<(LoginState, i64)>)] = &[
    // 内容违规：不是账号问题，不改状态
    (&["内容违规", "违反"], None),
    // 浏览器 / 选择器 / 图片模式错误：不是账号问题，不改状态
    (
        &[
            "浏览器错误",
            "生成超时",
            "timeout",
            "timed out",
            "未进入图片模式",
            "创建图片入口",
            "找不到元素",
            "查找元素失败",
            "未找到文本为",
            "selector",
            "underlying connection is closed",
            "tab closed",
            "session closed",
            "target navigated",
            "target closed",
        ],
        None,
    ),
    // 配额耗尽 / 账号禁用：直接禁用账号
    (
        &[
            "quota_exhausted",
            "额度耗尽",
            "free plan limit",
            "image generations requests",
            "limit resets in",
            "account disabled",
            "suspended",
            "banned",
            "封禁",
            "禁用",
        ],
        Some((LoginState::Disabled, 0)),
    ),
    // 登录失效
    (
        &["登录", "login", "logout", "expired", "from_logout"],
        Some((LoginState::LoginExpired, 0)),
    ),
    // 需要验证
    (
        &["verify", "验证"],
        Some((LoginState::VerificationRequired, 0)),
    ),
    // 限流 / 平台繁忙：不是明确账号失效，不改状态
    (&["rate limited", "rate limit", "频繁", "繁忙"], None),
];

fn classify_failure(error_msg: &str) -> Option<(LoginState, i64)> {
    let msg = error_msg.to_lowercase();
    for (keywords, result) in FAILURE_RULES {
        if keywords.iter().any(|kw| msg.contains(kw)) {
            return *result;
        }
    }
    // 未匹配到特定规则：默认不改账号状态，避免把流程类失败误判成账号异常
    None
}

fn classify_chatgpt_flow_failure(platform: &str, error_msg: &str) -> Option<(LoginState, i64)> {
    if platform != "chatgpt" {
        return None;
    }
    let msg = error_msg.to_lowercase();
    let image_mode_entry_failed = msg.contains("未进入图片模式") || msg.contains("创建图片入口");
    if image_mode_entry_failed {
        return Some((LoginState::Cooldown, 180));
    }
    None
}

fn extract_rate_limit_cooldown_hint(error_msg: &str) -> Option<i64> {
    let msg = error_msg.to_lowercase();
    let looks_like_rate_limit = msg.contains("rate_limit_exceeded")
        || msg.contains("you're generating images too quickly")
        || msg.contains("you’re generating images too quickly")
        || msg.contains("please wait for")
        || msg.contains("rate limited")
        || msg.contains("请求过于频繁")
        || msg.contains("生成过于频繁")
        || (msg.contains("请稍等") && msg.contains("分钟"))
        || (msg.contains("请等待") && msg.contains("分钟"));
    if !looks_like_rate_limit {
        return None;
    }

    let parse_after = |text: &str, marker: &str, unit_hint: &str| -> Option<i64> {
        let start = text.find(marker)? + marker.len();
        let rest = &text[start..];
        let digits: String = rest
            .chars()
            .skip_while(|c| c.is_whitespace())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let value = digits.parse::<i64>().ok()?;
        let unit_window: String = rest.chars().take(16).collect();
        if unit_window.contains("second") || unit_window.contains("秒") {
            return Some(value.max(1));
        }
        if unit_window.contains("minute") || unit_window.contains("分钟") || unit_hint == "minute"
        {
            return Some((value * 60).max(60));
        }
        Some(value.max(1))
    };

    parse_after(&msg, "please wait for ", "minute")
        .or_else(|| parse_after(&msg, "请等待", "minute"))
        .or_else(|| parse_after(&msg, "请稍等", "minute"))
        .or_else(|| parse_after(&msg, "等待", "minute"))
        // 命中了中文模糊文案"几分钟"时，默认冷却 3 分钟；英文无数字仍回退默认配置。
        .or_else(|| {
            if msg.contains("几分钟") {
                Some(180)
            } else {
                Some(-1)
            }
        })
}

fn truncate_reason(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

/// 任务失败后，根据错误信息分类并更新账号状态。
/// 在 api_server 的失败路径中调用，防止有问题的账号持续参与调度。
pub fn handle_task_failure(
    storage: &Storage,
    plugin_loader: &PluginLoader,
    account_id: &str,
    error: &AppError,
    step_results: &[StepResult],
) {
    // 从 error 和最后一个失败的 step_result 中提取错误信息
    let error_str = error.to_string();
    let step_error = step_results
        .iter()
        .rev()
        .find_map(|r| {
            if !r.success {
                r.error
                    .as_deref()
                    .or_else(|| r.data.get("error_msg").and_then(|v| v.as_str()))
            } else {
                None
            }
        })
        .unwrap_or("");
    let combined = format!("{} {}", error_str, step_error);
    let account_platform = storage
        .get_account(account_id)
        .ok()
        .flatten()
        .map(|a| a.platform)
        .unwrap_or_default();

    // 超时错误：不是明确账号异常，不改状态
    let is_timeout = matches!(error, AppError::GenerationTimeout);
    let classification = if is_timeout {
        None
    } else if let Some(cooldown_hint) = extract_rate_limit_cooldown_hint(&combined) {
        Some((LoginState::Cooldown, cooldown_hint))
    } else if let Some(v) = classify_chatgpt_flow_failure(&account_platform, &combined) {
        Some(v)
    } else {
        classify_failure(&combined)
    };

    let (state, cooldown_hint) = match classification {
        Some(v) => v,
        None => {
            tracing::info!(account_id, error = %error_str, "内容类错误，不更新账号状态");
            return;
        }
    };

    // 限流场景：从 manifest tier 读取 cooldown_seconds
    let cooldown_secs = if cooldown_hint == -1 {
        get_tier_cooldown(storage, plugin_loader, account_id)
    } else {
        cooldown_hint
    };

    let cooldown_until = if cooldown_secs > 0 {
        Some(Utc::now() + Duration::seconds(cooldown_secs))
    } else {
        None
    };

    let reason = format!("任务失败自动标记: {}", truncate_reason(&combined, 200));
    if let Err(e) =
        storage.update_login_state_with_cooldown(account_id, state.clone(), &reason, cooldown_until)
    {
        tracing::error!(account_id, error = %e, "更新账号失败状态出错");
        return;
    }

    tracing::warn!(
        account_id,
        new_state = ?state,
        cooldown_secs,
        "任务失败，已更新账号状态"
    );
}

/// 从 manifest 的 tier 配置中读取 cooldown_seconds；
/// tier/账号缺失时，回退到 settings 中的 `cooldown_default_seconds`（默认 300）。
fn get_tier_cooldown(storage: &Storage, plugin_loader: &PluginLoader, account_id: &str) -> i64 {
    let default_cd = read_default_cooldown_seconds(storage);
    let account = match storage.get_account(account_id) {
        Ok(Some(acc)) => acc,
        _ => return default_cd,
    };
    let manifest = match plugin_loader.get_plugin(&account.platform) {
        Some(m) => m,
        None => return default_cd,
    };
    let tier_key = if account.tier.is_empty() {
        "free"
    } else {
        &account.tier
    };
    manifest
        .tiers
        .get(tier_key)
        .map(|t| t.cooldown_seconds as i64)
        .unwrap_or(default_cd)
}

fn read_default_cooldown_seconds(storage: &Storage) -> i64 {
    storage
        .get_setting("cooldown_default_seconds")
        .ok()
        .flatten()
        .and_then(|v| v.trim_matches('"').parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .unwrap_or(300)
}

#[cfg(test)]
#[path = "account_manager_tests.rs"]
mod account_manager_tests;
