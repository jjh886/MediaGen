use std::collections::HashMap;

use mediagen_browser_gateway::{BROWSER_OS_METADATA_KEY, BROWSER_VERSION_METADATA_KEY};
use serde_json::{json, Value};

use crate::store::AccountRecord;

pub fn browser_metadata_from_account(account: &AccountRecord) -> HashMap<String, Value> {
    let mut metadata = HashMap::new();
    if !account.proxy_url.trim().is_empty() {
        metadata.insert("proxy_server".to_string(), json!(account.proxy_url));
    }
    if !account.proxy_id.trim().is_empty() {
        metadata.insert("proxy_id".to_string(), json!(account.proxy_id));
    }
    add_browser_profile_metadata(&mut metadata, &account.browser_os, &account.browser_version);
    metadata
}

pub fn add_browser_profile_metadata(
    metadata: &mut HashMap<String, Value>,
    browser_os: &str,
    browser_version: &str,
) {
    let browser_os = browser_os.trim();
    let browser_version = browser_version.trim();
    if browser_os.is_empty() || browser_version.is_empty() {
        return;
    }
    // 前后端共用 metadata key，避免账号保存和浏览器启动各写一套字段名。
    metadata.insert(BROWSER_OS_METADATA_KEY.to_string(), json!(browser_os));
    metadata.insert(
        BROWSER_VERSION_METADATA_KEY.to_string(),
        json!(browser_version),
    );
}

#[cfg(test)]
mod tests {
    use super::{add_browser_profile_metadata, browser_metadata_from_account};
    use crate::test_fixtures::account_record;
    use std::collections::HashMap;

    #[test]
    fn account_metadata_keeps_proxy_and_browser_profile() {
        let mut account = account_record("acc-1", "codex", "Codex", "");
        account.proxy_url = "socks5h://proxy.example:1080".to_string();
        account.browser_os = "windows".to_string();
        account.browser_version = "126".to_string();

        let metadata = browser_metadata_from_account(&account);

        assert_eq!(metadata["proxy_server"], "socks5h://proxy.example:1080");
        assert_eq!(metadata["browser_os"], "windows");
        assert_eq!(metadata["browser_version"], "126");
    }

    #[test]
    fn browser_profile_metadata_requires_complete_profile() {
        let mut metadata = HashMap::new();

        add_browser_profile_metadata(&mut metadata, "windows", " ");
        assert!(metadata.is_empty());

        add_browser_profile_metadata(&mut metadata, "macos", "127");
        assert_eq!(metadata["browser_os"], "macos");
        assert_eq!(metadata["browser_version"], "127");
    }
}
