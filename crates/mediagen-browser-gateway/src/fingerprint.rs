use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::cdp::CdpClient;
use crate::{BrowserGatewayError, BrowserSession};

pub const BROWSER_OS_METADATA_KEY: &str = "browser_os";
pub const BROWSER_VERSION_METADATA_KEY: &str = "browser_version";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserFingerprint {
    os: BrowserOs,
    chrome_major_version: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserOs {
    Windows,
    Macos,
    Linux,
}

impl BrowserFingerprint {
    pub fn from_values(os: &str, browser_version: &str) -> Option<Self> {
        let os = BrowserOs::parse(os)?;
        let chrome_major_version = parse_chrome_major_version(browser_version)?;
        Some(Self {
            os,
            chrome_major_version,
        })
    }

    pub fn from_metadata(metadata: &HashMap<String, Value>) -> Option<Self> {
        Self::from_values(
            metadata.get(BROWSER_OS_METADATA_KEY)?.as_str()?,
            metadata.get(BROWSER_VERSION_METADATA_KEY)?.as_str()?,
        )
    }

    pub fn user_agent(&self) -> String {
        format!(
            "Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
            self.os.ua_token(),
            self.chrome_major_version
        )
    }

    pub fn sec_ch_ua(&self) -> String {
        format!(
            "\"Chromium\";v=\"{}\", \"Google Chrome\";v=\"{}\", \"Not A(Brand\";v=\"99\"",
            self.chrome_major_version, self.chrome_major_version
        )
    }

    pub fn sec_ch_ua_platform(&self) -> &'static str {
        self.os.ua_ch_platform()
    }

    fn user_agent_metadata(&self) -> Value {
        let version = self.chrome_major_version.to_string();
        let full_version = format!("{}.0.0.0", self.chrome_major_version);
        json!({
            "brands": [
                { "brand": "Chromium", "version": version },
                { "brand": "Google Chrome", "version": version },
                { "brand": "Not A(Brand", "version": "99" }
            ],
            "fullVersionList": [
                { "brand": "Chromium", "version": full_version },
                { "brand": "Google Chrome", "version": full_version },
                { "brand": "Not A(Brand", "version": "99.0.0.0" }
            ],
            "fullVersion": full_version,
            "platform": self.os.ua_ch_platform(),
            "platformVersion": self.os.platform_version(),
            "architecture": "x86",
            "model": "",
            "mobile": false,
            "bitness": "64"
        })
    }

    fn navigator_platform(&self) -> &'static str {
        self.os.navigator_platform()
    }
}

impl BrowserOs {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "windows" | "win" | "win32" => Some(Self::Windows),
            "macos" | "mac" | "darwin" => Some(Self::Macos),
            "linux" => Some(Self::Linux),
            _ => None,
        }
    }

    fn ua_token(self) -> &'static str {
        match self {
            Self::Windows => "Windows NT 10.0; Win64; x64",
            Self::Macos => "Macintosh; Intel Mac OS X 10_15_7",
            Self::Linux => "X11; Linux x86_64",
        }
    }

    fn ua_ch_platform(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Macos => "macOS",
            Self::Linux => "Linux",
        }
    }

    fn navigator_platform(self) -> &'static str {
        match self {
            Self::Windows => "Win32",
            Self::Macos => "MacIntel",
            Self::Linux => "Linux x86_64",
        }
    }

    fn platform_version(self) -> &'static str {
        match self {
            Self::Windows => "10.0.0",
            Self::Macos => "10.15.7",
            Self::Linux => "6.0.0",
        }
    }
}

pub(crate) async fn apply_browser_fingerprint(
    client: &Arc<CdpClient>,
    session: &BrowserSession,
) -> Result<(), BrowserGatewayError> {
    let Some(fingerprint) = BrowserFingerprint::from_metadata(&session.metadata) else {
        return Ok(());
    };
    // CDP 的 UA override 必须在首次导航前设置，后续页面请求才会带上账号配置。
    let override_result = client
        .send(
            "Network.setUserAgentOverride",
            json!({
                "userAgent": fingerprint.user_agent(),
                "platform": fingerprint.navigator_platform(),
                "userAgentMetadata": fingerprint.user_agent_metadata()
            }),
        )
        .await;
    if override_result.is_err() {
        client
            .send(
                "Network.setUserAgentOverride",
                json!({
                    "userAgent": fingerprint.user_agent(),
                    "platform": fingerprint.navigator_platform()
                }),
            )
            .await?;
    }
    client
        .send(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({
                "source": format!(
                    "Object.defineProperty(Navigator.prototype, 'platform', {{ get: () => '{}', configurable: true }});",
                    fingerprint.navigator_platform()
                )
            }),
        )
        .await?;
    Ok(())
}

fn parse_chrome_major_version(value: &str) -> Option<u16> {
    let major = value.trim().split('.').next()?.parse::<u16>().ok()?;
    (major > 0).then_some(major)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_stable_user_agent_for_selected_profile() {
        let profile = BrowserFingerprint::from_values("windows", "126").unwrap();

        assert!(profile.user_agent().contains("Windows NT 10.0; Win64; x64"));
        assert!(profile.user_agent().contains("Chrome/126.0.0.0"));
        assert_eq!(profile.sec_ch_ua_platform(), "Windows");
    }

    #[test]
    fn ignores_incomplete_or_unknown_profile() {
        assert!(BrowserFingerprint::from_values("windows", "").is_none());
        assert!(BrowserFingerprint::from_values("android", "126").is_none());
    }
}
