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

pub fn groups_for_key(settings_keys: &str, key: &str) -> Vec<String> {
    setting_key_specs(settings_keys)
        .filter(|spec| spec.key == key)
        .flat_map(|spec| spec.groups)
        .collect()
}

pub fn key_grants(settings_keys: &str) -> Vec<(String, Vec<String>)> {
    setting_key_specs(settings_keys)
        .map(|spec| (spec.key.to_string(), spec.groups))
        .collect()
}

pub fn account_key_allowed(account_keys: &str, key: &str) -> bool {
    account_keys
        .lines()
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .any(|configured| !configured.is_empty() && configured == key)
}

struct KeySpec<'a> {
    key: &'a str,
    groups: Vec<String>,
}

fn setting_key_specs(value: &str) -> impl Iterator<Item = KeySpec<'_>> {
    value.lines().filter_map(|line| {
        let text = line.trim();
        if text.is_empty() {
            return None;
        }
        let (key, groups) = text
            .split_once('=')
            .or_else(|| text.split_once('|'))
            .unwrap_or((text, ""));
        Some(KeySpec {
            key: key.trim(),
            groups: split_groups(groups),
        })
    })
}

fn split_groups(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_global_api_key_groups() {
        let settings = "key-a=默认分组,vip\nkey-b|team-b\nplain";
        assert!(settings_key_allowed(settings, "key-a"));
        assert_eq!(groups_for_key(settings, "key-a"), vec!["默认分组", "vip"]);
        assert_eq!(groups_for_key(settings, "key-b"), vec!["team-b"]);
        assert!(groups_for_key(settings, "plain").is_empty());
    }

    #[test]
    fn account_keys_are_strict() {
        assert!(account_key_allowed("key-a\nkey-b,key-c", "key-c"));
        assert!(!account_key_allowed("", "key-a"));
        assert!(!account_key_allowed("key-a", "key-x"));
    }
}
