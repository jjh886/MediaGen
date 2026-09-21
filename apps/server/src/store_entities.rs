use std::sync::Mutex;

use chrono::Utc;
use uuid::Uuid;

use crate::access_control::{
    account_key_allowed, hash_account_keys, hash_secret, settings_key_allowed,
};
use crate::store::AppStore;
use crate::store_models::{ApiKeyRecord, AuditLogRecord, ProxyRecord, StoreSnapshot};

impl AppStore {
    pub fn api_key_allowed(&self, key: &str) -> bool {
        self.global_api_key_allowed(key) || self.account_api_key_allowed(key)
    }

    pub fn global_api_key_allowed(&self, key: &str) -> bool {
        let hash = hash_secret(key);
        let keys = self.api_keys.lock().unwrap_or_else(|err| err.into_inner());
        if keys.iter().any(|record| record.key_hash == hash) {
            return true;
        }
        drop(keys);

        let settings = self.settings.lock().unwrap_or_else(|err| err.into_inner());
        settings_key_allowed(&settings.api_keys, key)
    }

    fn account_api_key_allowed(&self, key: &str) -> bool {
        self.accounts
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .any(|account| account_key_allowed(&account.api_key, key))
    }

    pub fn list_api_keys(&self) -> Vec<ApiKeyRecord> {
        self.api_keys
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn create_api_key(&self, name: String, key: String) -> ApiKeyRecord {
        let record = api_key_record(name, key);
        self.api_keys
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(record.clone());
        self.save();
        record
    }

    pub fn delete_api_key(&self, id: &str) -> bool {
        let changed = retain_by_id(&self.api_keys, id);
        if changed {
            self.save();
        }
        changed
    }

    pub fn list_proxies(&self) -> Vec<ProxyRecord> {
        self.proxies
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn proxy_by_id(&self, id: &str) -> Option<ProxyRecord> {
        self.proxies
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .find(|proxy| proxy.id == id)
            .cloned()
    }

    pub fn create_proxy(&self, proxy: ProxyRecord) -> ProxyRecord {
        self.proxies
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(proxy.clone());
        self.save();
        proxy
    }

    pub fn delete_proxy(&self, id: &str) -> bool {
        let changed = retain_by_id(&self.proxies, id);
        if changed {
            self.save();
        }
        changed
    }

    pub fn update_proxy(&self, proxy: ProxyRecord) -> Option<ProxyRecord> {
        let mut proxies = self.proxies.lock().unwrap_or_else(|e| e.into_inner());
        let current = proxies.iter_mut().find(|p| p.id == proxy.id)?;
        *current = proxy.clone();
        drop(proxies);
        self.save();
        Some(proxy)
    }

    pub fn list_audit_logs(&self) -> Vec<AuditLogRecord> {
        let mut logs = self
            .audit_logs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        logs
    }

    pub fn add_audit_log(
        &self,
        module: &str,
        level: &str,
        summary: String,
        detail: String,
        task_id: Option<String>,
    ) {
        self.audit_logs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(audit_log(module, level, summary, detail, task_id));
        self.save();
    }

    pub fn clear_audit_logs(&self) {
        self.audit_logs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
        self.save();
    }

    pub(crate) fn replace_api_keys_from_settings_update(&self, raw: Option<&str>) {
        if let Some(raw) = raw {
            let records = api_key_records_from_lines(raw);
            let mut keys = self.api_keys.lock().unwrap_or_else(|err| err.into_inner());
            if !records.is_empty() {
                *keys = records;
            }
        }
    }
}

pub(crate) fn migrate_legacy_api_keys(snapshot: &mut StoreSnapshot) {
    if snapshot.api_keys.is_empty() && !snapshot.settings.api_keys.trim().is_empty() {
        snapshot.api_keys = api_key_records_from_lines(&snapshot.settings.api_keys);
        snapshot.settings.api_keys.clear();
    }
    for account in &mut snapshot.accounts {
        account.api_key = hash_account_keys(&account.api_key);
    }
}

fn api_key_records_from_lines(raw: &str) -> Vec<ApiKeyRecord> {
    raw.lines().filter_map(api_key_record_from_line).collect()
}

fn api_key_record_from_line(line: &str) -> Option<ApiKeyRecord> {
    let text = line.trim();
    if text.is_empty() {
        return None;
    }
    let (key, _) = text
        .split_once('=')
        .or_else(|| text.split_once('|'))
        .unwrap_or((text, ""));
    Some(api_key_record(
        key.trim().to_string(),
        key.trim().to_string(),
    ))
}

fn api_key_record(name: String, key: String) -> ApiKeyRecord {
    ApiKeyRecord {
        id: format!("key-{}", Uuid::new_v4()),
        name: first_non_empty(&[name.as_str(), "API Key"]),
        key_hash: hash_secret(&key),
        key_prefix: key.chars().take(4).collect(),
        key_suffix: last_chars(&key, 4),
        rpm_limit: 0,
        daily_limit: 0,
        expires_at: None,
        created_at: Utc::now().to_rfc3339(),
    }
}

fn audit_log(
    module: &str,
    level: &str,
    summary: String,
    detail: String,
    task_id: Option<String>,
) -> AuditLogRecord {
    AuditLogRecord {
        id: format!("log-{}", Uuid::new_v4()),
        timestamp: Utc::now().to_rfc3339(),
        module: module.to_string(),
        level: level.to_string(),
        summary,
        detail,
        task_id,
    }
}

fn retain_by_id<T>(items: &Mutex<Vec<T>>, id: &str) -> bool
where
    T: HasRecordId,
{
    let mut items = items.lock().unwrap_or_else(|err| err.into_inner());
    let before = items.len();
    items.retain(|item| item.record_id() != id);
    items.len() != before
}

trait HasRecordId {
    fn record_id(&self) -> &str;
}

impl HasRecordId for ApiKeyRecord {
    fn record_id(&self) -> &str {
        &self.id
    }
}

impl HasRecordId for ProxyRecord {
    fn record_id(&self) -> &str {
        &self.id
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn last_chars(value: &str, count: usize) -> String {
    value
        .chars()
        .rev()
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        api_key_record_from_line, first_non_empty, hash_secret, last_chars, migrate_legacy_api_keys,
    };
    use crate::store_models::StoreSnapshot;

    #[test]
    fn api_key_line_parser_discards_legacy_suffix() {
        let equals = api_key_record_from_line(" key-a = vip, default ").unwrap();
        let pipe = api_key_record_from_line("key-b|team").unwrap();

        assert_eq!(equals.name, "key-a");
        assert_eq!(equals.key_prefix, "key-");
        assert_eq!(equals.key_suffix, "ey-a");
        assert_eq!(pipe.name, "key-b");
        assert!(api_key_record_from_line("  ").is_none());
    }

    #[test]
    fn legacy_api_keys_migrate_to_hash_records() {
        let mut snapshot = StoreSnapshot::default();
        snapshot.settings.api_keys = "legacy-key=vip".to_string();

        migrate_legacy_api_keys(&mut snapshot);

        assert!(snapshot.settings.api_keys.is_empty());
        assert_eq!(snapshot.api_keys.len(), 1);
        assert_eq!(snapshot.api_keys[0].key_hash, hash_secret("legacy-key"));
    }

    #[test]
    fn text_helpers_normalize_empty_and_short_values() {
        assert_eq!(first_non_empty(&[" ", "fallback"]), "fallback");
        assert_eq!(last_chars("abc", 8), "abc");
    }
}
