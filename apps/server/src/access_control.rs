use sha2::{Digest, Sha256};

const SECRET_HASH_PREFIX: &str = "sha256:";

pub fn normalize_lines(value: &str) -> String {
    value
        .lines()
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn normalize_setting_keys(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn settings_key_allowed(settings_keys: &str, key: &str) -> bool {
    setting_key_specs(settings_keys).any(|spec| spec.key == key)
}

pub fn account_key_allowed(account_keys: &str, key: &str) -> bool {
    let hashed = hash_secret_token(key);
    account_keys
        .lines()
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .any(|configured| !configured.is_empty() && (configured == key || configured == hashed))
}

pub fn hash_account_keys(value: &str) -> String {
    normalize_lines(value)
        .lines()
        .map(hash_account_key_line)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn mask_account_keys(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(mask_account_key_line)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn hash_secret(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn hash_secret_token(value: &str) -> String {
    format!("{SECRET_HASH_PREFIX}{}", hash_secret(value))
}

fn hash_account_key_line(value: &str) -> String {
    if value.starts_with(SECRET_HASH_PREFIX) {
        value.to_string()
    } else {
        hash_secret_token(value)
    }
}

fn mask_account_key_line(value: &str) -> String {
    if value.starts_with(SECRET_HASH_PREFIX) {
        return "已保存".to_string();
    }
    let prefix = value.chars().take(4).collect::<String>();
    let suffix = last_chars(value, 4);
    format!("{prefix}****{suffix}")
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

struct KeySpec<'a> {
    key: &'a str,
}

fn setting_key_specs(value: &str) -> impl Iterator<Item = KeySpec<'_>> {
    value.lines().filter_map(|line| {
        let text = line.trim();
        if text.is_empty() {
            return None;
        }
        let (key, _) = text
            .split_once('=')
            .or_else(|| text.split_once('|'))
            .unwrap_or((text, ""));
        Some(KeySpec { key: key.trim() })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_suffix_does_not_affect_global_api_key_auth() {
        let settings = "key-a=legacy\nkey-b|team-b\nplain";
        assert!(settings_key_allowed(settings, "key-a"));
        assert!(settings_key_allowed(settings, "key-b"));
        assert!(settings_key_allowed(settings, "plain"));
    }

    #[test]
    fn account_keys_are_strict() {
        assert!(account_key_allowed("key-a\nkey-b,key-c", "key-c"));
        assert!(account_key_allowed(&hash_account_keys("key-a"), "key-a"));
        assert!(!account_key_allowed("", "key-a"));
        assert!(!account_key_allowed("key-a", "key-x"));
    }
}
