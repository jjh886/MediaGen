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
}

/// 按 settings 中的 `platform_tier_overrides` 覆盖 manifest 默认 TierDef。
/// 键格式 `{platform}_{tier_key}`，例如 "doubao_free"、"flow_free"。
/// 仅覆盖 daily_limit / total_limit / concurrency_limit 三项；其它字段保持。
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
    }
    tier
}

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }

    #[test]
    fn no_setting_returns_original() {
        let (_d, s) = fresh_storage();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 50);
        assert_eq!(out.total_limit, 0);
        assert_eq!(out.concurrency_limit, 1);
    }

    #[test]
    fn empty_json_object_returns_original() {
        let (_d, s) = fresh_storage();
        s.set_setting("platform_tier_overrides", "{}").unwrap();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 50);
        assert_eq!(out.concurrency_limit, 1);
    }

    #[test]
    fn malformed_json_warns_and_returns_original() {
        let (_d, s) = fresh_storage();
        s.set_setting("platform_tier_overrides", "{not json")
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
    fn full_override_replaces_all_three_fields() {
        let (_d, s) = fresh_storage();
        s.set_setting(
            "platform_tier_overrides",
            r#"{"flow_free":{"daily_limit":80,"total_limit":1000,"concurrency_limit":3}}"#,
        )
        .unwrap();
        let out = apply_overrides(&s, "flow", "free", base_tier());
        assert_eq!(out.daily_limit, 80);
        assert_eq!(out.total_limit, 1000);
        assert_eq!(out.concurrency_limit, 3);
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
}
