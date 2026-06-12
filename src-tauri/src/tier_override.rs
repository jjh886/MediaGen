use std::collections::HashMap;

use serde::Deserialize;

use crate::storage::Storage;
use crate::types::TierDef;

#[derive(Debug, Default, Deserialize)]
struct TierOverride {
    #[serde(default)]
    daily_limit: Option<i32>,
    #[serde(default)]
    total_limit: Option<i32>,
    #[serde(default)]
    concurrency_limit: Option<i32>,
    #[serde(default)]
    task_interval_seconds: Option<i32>,
}

/// 按 settings 中的 `platform_tier_overrides` 覆盖 manifest 默认 TierDef。
/// 键格式 `{platform}_{tier_key}`，例如 "provider_free"。
/// 仅覆盖限额、并发和任务间隔；其它字段保持。
/// 解析失败 / 缺少 key / 部分字段缺失都安全降级为原值。
pub fn apply_overrides(
    storage: &Storage,
    platform: &str,
    tier_key: &str,
    mut tier: TierDef,
) -> TierDef {
    let raw = match storage.get_setting("platform_tier_overrides") {
        Ok(Some(s)) if !s.trim().is_empty() => s,
        _ => return tier,
    };
    let parsed: HashMap<String, TierOverride> = match serde_json::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "platform_tier_overrides 解析失败，沿用 manifest 默认值");
            return tier;
        }
    };
    let key = format!("{}_{}", platform, tier_key);
    if let Some(o) = parsed.get(&key) {
        if let Some(v) = o.daily_limit {
            tier.daily_limit = v;
        }
        if let Some(v) = o.total_limit {
            tier.total_limit = v;
        }
        if let Some(v) = o.concurrency_limit {
            tier.concurrency_limit = v;
        }
        if let Some(v) = o.task_interval_seconds {
            tier.task_interval_seconds = v;
        }
    }
    tier
}

pub fn apply_overrides_to_existing_accounts(
    storage: &Storage,
    raw: &str,
) -> Result<usize, crate::error::AppError> {
    let parsed: HashMap<String, TierOverride> = match serde_json::from_str(raw) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "platform_tier_overrides 解析失败，跳过已有账号同步");
            return Ok(0);
        }
    };
    let mut updated = 0;
    for (key, override_def) in parsed {
        let Some((platform, tier_key)) = key.rsplit_once('_') else {
            continue;
        };
        for mut account in storage.list_accounts_by_platform(platform)? {
            if account.tier != tier_key {
                continue;
            }
            let mut changed = false;
            if let Some(v) = override_def.daily_limit {
                changed |= account.daily_limit != v;
                account.daily_limit = v;
            }
            if let Some(v) = override_def.total_limit {
                changed |= account.total_limit != v;
                account.total_limit = v;
            }
            if let Some(v) = override_def.concurrency_limit {
                changed |= account.concurrency_limit != v;
                account.concurrency_limit = v;
            }
            if let Some(v) = override_def.task_interval_seconds {
                changed |= account.task_interval_seconds != v;
                account.task_interval_seconds = v;
            }
            if changed {
                storage.update_account(&account)?;
                updated += 1;
            }
        }
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PlatformAccount;
    use tempfile::tempdir;

    fn fresh_storage() -> (tempfile::TempDir, Storage) {
        let dir = tempdir().unwrap();
        let storage = Storage::new(&dir.path().join("test.db")).unwrap();
        (dir, storage)
    }

    fn base_tier() -> TierDef {
        TierDef {
            display_name: "免费用户".into(),
            is_default: true,
            concurrency_limit: 1,
            daily_limit: 50,
            total_limit: 0,
            cooldown_seconds: 30,
            task_interval_seconds: 0,
        }
    }

    fn sample_account(id: &str, platform: &str, tier: &str) -> PlatformAccount {
        let mut account = crate::types::platform_account_fixture(id, platform);
        account.tier = tier.to_string();
        account.daily_limit = 50;
        account.task_interval_seconds = 0;
        account
    }

    #[test]
    fn no_setting_returns_original() {
        let (_d, s) = fresh_storage();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 50);
        assert_eq!(out.total_limit, 0);
        assert_eq!(out.concurrency_limit, 1);
        assert_eq!(out.task_interval_seconds, 0);
    }

    #[test]
    fn empty_json_object_returns_original() {
        let (_d, s) = fresh_storage();
        s.set_setting("platform_tier_overrides", "{}").unwrap();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 50);
        assert_eq!(out.concurrency_limit, 1);
        assert_eq!(out.task_interval_seconds, 0);
    }

    #[test]
    fn malformed_json_warns_and_returns_original() {
        let (_d, s) = fresh_storage();
        s.set_setting("platform_tier_overrides", "not json")
            .unwrap();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 50);
    }

    #[test]
    fn key_mismatch_returns_original() {
        let (_d, s) = fresh_storage();
        s.set_setting(
            "platform_tier_overrides",
            r#"{"doubao_free":{"daily_limit":100}}"#,
        )
        .unwrap();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 50);
    }

    #[test]
    fn full_override_replaces_all_supported_fields() {
        let (_d, s) = fresh_storage();
        s.set_setting(
            "platform_tier_overrides",
            r#"{"flow_free":{"daily_limit":80,"total_limit":1000,"concurrency_limit":3,"task_interval_seconds":45}}"#,
        )
        .unwrap();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 80);
        assert_eq!(out.total_limit, 1000);
        assert_eq!(out.concurrency_limit, 3);
        assert_eq!(out.task_interval_seconds, 45);
    }

    #[test]
    fn partial_override_keeps_unspecified_fields() {
        let (_d, s) = fresh_storage();
        s.set_setting(
            "platform_tier_overrides",
            r#"{"flow_free":{"concurrency_limit":5}}"#,
        )
        .unwrap();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 50);
        assert_eq!(out.total_limit, 0);
        assert_eq!(out.concurrency_limit, 5);
    }

    #[test]
    fn other_unrelated_fields_in_tier_def_are_preserved() {
        let (_d, s) = fresh_storage();
        s.set_setting(
            "platform_tier_overrides",
            r#"{"flow_free":{"daily_limit":80}}"#,
        )
        .unwrap();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.cooldown_seconds, 30);
        assert_eq!(out.display_name, "免费用户");
        assert!(out.is_default);
    }

    #[test]
    fn applies_overrides_to_existing_matching_accounts() {
        let (_d, s) = fresh_storage();
        s.insert_account(&sample_account("a1", "flow", "free"))
            .unwrap();
        s.insert_account(&sample_account("a2", "flow", "pro"))
            .unwrap();

        let updated = apply_overrides_to_existing_accounts(
            &s,
            r#"{"flow_free":{"daily_limit":80,"total_limit":1000,"concurrency_limit":0,"task_interval_seconds":30}}"#,
        )
        .unwrap();

        let free = s.get_account("a1").unwrap().unwrap();
        let pro = s.get_account("a2").unwrap().unwrap();
        assert_eq!(updated, 1);
        assert_eq!(free.daily_limit, 80);
        assert_eq!(free.total_limit, 1000);
        assert_eq!(free.concurrency_limit, 0);
        assert_eq!(free.task_interval_seconds, 30);
        assert_eq!(pro.daily_limit, 50);
    }

    #[test]
    fn malformed_existing_override_json_is_ignored() {
        let (_d, s) = fresh_storage();
        s.insert_account(&sample_account("a1", "flow", "free"))
            .unwrap();

        let updated = apply_overrides_to_existing_accounts(&s, "not json").unwrap();

        let account = s.get_account("a1").unwrap().unwrap();
        assert_eq!(updated, 0);
        assert_eq!(account.daily_limit, 50);
    }
}
