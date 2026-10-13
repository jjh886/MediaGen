use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
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

    pub(crate) fn launch_args(&self, profile: &Path, native_major: Option<u16>) -> Vec<String> {
        let hash = Sha256::digest(profile.as_os_str().to_string_lossy().as_bytes());
        let seed = (u32::from_le_bytes(hash[..4].try_into().unwrap()) & 0x7fff_ffff).max(1);
        let mut args = vec![
            format!("--fingerprint={seed}"),
            format!("--fingerprint-platform={}", self.os.launch_platform()),
        ];
        if native_major != Some(self.chrome_major_version)
            || self.os.launch_platform() != std::env::consts::OS
        {
            args.extend([
                "--fingerprint-brand=Chrome".into(),
                format!("--user-agent={}", self.user_agent()),
                format!(
                    "--fingerprint-brand-version={}.0.0.0",
                    self.chrome_major_version
                ),
            ]);
        }
        args
    }

    pub(crate) fn checked_metadata(
        metadata: &HashMap<String, Value>,
    ) -> Result<Option<Self>, BrowserGatewayError> {
        let empty = |key| {
            metadata
                .get(key)
                .is_none_or(|value| value.as_str().is_some_and(|text| text.trim().is_empty()))
        };
        if empty(BROWSER_OS_METADATA_KEY) && empty(BROWSER_VERSION_METADATA_KEY) {
            return Ok(None);
        }
        Self::from_metadata(metadata).map(Some).ok_or_else(|| {
            BrowserGatewayError::BrowserLaunch(
                "浏览器环境无效：请选择有效的系统和 Chrome 主版本".into(),
            )
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

    fn launch_platform(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }
}

pub(crate) async fn apply_browser_fingerprint(
    client: &Arc<CdpClient>,
    session: &BrowserSession,
) -> Result<(), BrowserGatewayError> {
    let Some(fingerprint) = BrowserFingerprint::checked_metadata(&session.metadata)? else {
        return Ok(());
    };
    // 身份从启动阶段设置，不能再用页面级 CDP override 造成 Worker / 请求头不一致。
    let actual = client
        .send(
            "Runtime.evaluate",
            json!({
                "expression": "({ua:navigator.userAgent,platform:navigator.platform})",
                "returnByValue": true
            }),
        )
        .await?;
    let value = &actual["result"]["value"];
    if !value["ua"]
        .as_str()
        .unwrap_or_default()
        .contains(&format!("Chrome/{}.", fingerprint.chrome_major_version))
        || value["platform"] != fingerprint.navigator_platform()
    {
        return Err(BrowserGatewayError::BrowserLaunch(
            "浏览器内核未应用所选环境，请修复浏览器组件或选择本机系统和实际内核；不会使用不一致的版本伪装继续登录".into()
        ));
    }
    Ok(())
}

fn parse_chrome_major_version(value: &str) -> Option<u16> {
    let major = value.trim().split('.').next()?.parse::<u16>().ok()?;
    (100..=999).contains(&major).then_some(major)
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
    #[test]
    fn launch_identity_is_stable_and_native_version_is_preserved() {
        let simulated = BrowserFingerprint::from_values("windows", "155").unwrap();
        let first = simulated.launch_args(Path::new("/profiles/a"), Some(145));
        assert_eq!(
            first,
            simulated.launch_args(Path::new("/profiles/a"), Some(145))
        );
        assert_ne!(
            first[0],
            simulated.launch_args(Path::new("/profiles/b"), Some(145))[0]
        );
        assert!(first.contains(&"--fingerprint-brand-version=155.0.0.0".into()));
        assert!(first.contains(&format!("--user-agent={}", simulated.user_agent())));
        let native = BrowserFingerprint::from_values(std::env::consts::OS, "145").unwrap();
        assert_eq!(
            native
                .launch_args(Path::new("/profiles/a"), Some(145))
                .len(),
            2
        );
        assert!(BrowserFingerprint::checked_metadata(&HashMap::from([(
            BROWSER_VERSION_METADATA_KEY.into(),
            json!("145")
        )]))
        .is_err());
        assert!(BrowserFingerprint::from_values("macos", "1").is_none());
    }
}
