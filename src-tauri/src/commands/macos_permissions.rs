use serde::Serialize;

/// 启动前需要确认的 macOS 权限项。
///
/// App Management 属于 macOS 13+ 的 `SystemPolicyAppBundles` 隐私项。
/// Apple 没有提供类似辅助功能 `AXIsProcessTrusted` 的公开查询 API；为了避免破坏
/// Chrome.app 签名，这里只提供明确的启动前引导，不做“试写 app bundle”的危险探测。
#[derive(Debug, Clone, Serialize)]
pub struct MacosPermissionItem {
    pub key: &'static str,
    pub title: &'static str,
    pub granted: bool,
    pub blocking: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MacosStartupPermissionStatus {
    pub required: bool,
    pub granted: bool,
    pub permissions: Vec<MacosPermissionItem>,
}

#[tauri::command]
pub async fn check_macos_startup_permissions(
    prompt: bool,
) -> Result<MacosStartupPermissionStatus, String> {
    let _ = prompt;
    Ok(check_macos_startup_permissions_inner(prompt))
}

#[tauri::command]
pub async fn open_macos_privacy_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        open_privacy_settings()
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

fn check_macos_startup_permissions_inner(prompt: bool) -> MacosStartupPermissionStatus {
    #[cfg(target_os = "macos")]
    {
        let _ = prompt;
        let permissions = vec![app_management_permission()];
        let granted = permissions
            .iter()
            .filter(|item| item.blocking)
            .all(|item| item.granted);
        MacosStartupPermissionStatus {
            required: true,
            granted,
            permissions,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = prompt;
        MacosStartupPermissionStatus {
            required: false,
            granted: true,
            permissions: Vec::new(),
        }
    }
}

#[cfg(target_os = "macos")]
fn open_privacy_settings() -> Result<(), String> {
    let targets = [
        // App Management 在 macOS 13+ 对应 SystemPolicyAppBundles；系统版本不同，
        // URL anchor 支持不完全一致，所以按“精确项 -> 隐私总入口”逐级兜底。
        "x-apple.systempreferences:com.apple.preference.security?Privacy_AppBundles",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_AppManagement",
        "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension",
        "x-apple.systempreferences:com.apple.settings.PrivacySecurity",
        "x-apple.systempreferences:com.apple.preference.security?Privacy",
    ];

    for target in targets {
        if std::process::Command::new("open")
            .arg(target)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }

    std::process::Command::new("open")
        .arg("-b")
        .arg("com.apple.systempreferences")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法打开系统设置: {e}"))
}

#[cfg(target_os = "macos")]
fn app_management_permission() -> MacosPermissionItem {
    MacosPermissionItem {
        key: "app_management",
        title: "App 管理权限",
        granted: false,
        blocking: true,
        message:
            "MediaGen 需要这个权限来让浏览器相关组件稳定启动，避免任务执行时才被 macOS 拦截并要求重启应用。"
                .to_string(),
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::check_macos_startup_permissions_inner;

    #[test]
    fn non_macos_status_never_blocks_startup() {
        let status = check_macos_startup_permissions_inner(false);
        assert!(!status.required);
        assert!(status.granted);
        assert!(status.permissions.is_empty());
    }
}
