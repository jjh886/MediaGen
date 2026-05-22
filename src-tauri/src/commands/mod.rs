pub mod account;
mod account_plugin_flow;

use std::sync::Arc;

use crate::sync_ext::MutexExt;
use crate::types::*;
use crate::AppState;

pub(crate) use account::{default_chrome_candidates, resolve_chrome_path};

#[tauri::command]
pub fn get_app_info() -> String {
    "frame64-desktop v0.1.0".to_string()
}

#[tauri::command]
pub async fn get_plugins(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    let list: Vec<serde_json::Value> = state
        .plugin_loader
        .sorted_plugins()
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "platform_id": m.platform_id,
                "display_name": m.display_name,
                "model_count": m.models.len(),
                "supports_registration": m.registration.as_ref().map(|r| r.enabled).unwrap_or(false),
            })
        })
        .collect();
    Ok(list)
}

#[tauri::command]
pub async fn get_accounts(
    state: tauri::State<'_, Arc<AppState>>,
    platform: String,
) -> Result<Vec<PlatformAccount>, String> {
    let storage = state.storage.lock_recover();
    storage
        .list_accounts_by_platform(&platform)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_account(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let storage = state.storage.lock_recover();
    let profile_dir = storage
        .get_account(&id)
        .map_err(|e| e.to_string())?
        .and_then(|a| {
            if a.profile_dir.is_empty() {
                None
            } else {
                Some(a.profile_dir)
            }
        });
    storage.delete_account(&id).map_err(|e| e.to_string())?;
    drop(storage);
    if let Some(dir) = profile_dir {
        // 删除 profile 前必须先释放可能常驻的 Chrome 实例，否则 Windows 上会因文件占用删不干净
        state.browser_engine.force_release_profile(&dir);
        if std::path::Path::new(&dir).exists() {
            tracing::info!("删除 profile 目录: {}", dir);
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::warn!("删除 profile 目录失败: {}", e);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn update_account(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
    tier: Option<String>,
    concurrency_limit: Option<i32>,
    daily_limit: Option<i32>,
    total_limit: Option<i32>,
) -> Result<(), String> {
    let storage = state.storage.lock_recover();
    let mut account = storage
        .get_account(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("账号 {id} 不存在"))?;
    if let Some(v) = tier {
        account.tier = v;
    }
    if let Some(v) = concurrency_limit {
        account.concurrency_limit = v;
    }
    if let Some(v) = daily_limit {
        account.daily_limit = v;
    }
    if let Some(v) = total_limit {
        account.total_limit = v;
    }
    storage.update_account(&account).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn toggle_account(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
    disabled: bool,
) -> Result<(), String> {
    let storage = state.storage.lock_recover();
    let new_state = if disabled {
        crate::types::LoginState::Disabled
    } else {
        crate::types::LoginState::Available
    };
    let reason = if disabled {
        "手动禁用"
    } else {
        "手动启用"
    };
    storage
        .update_login_state(&id, new_state, reason)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_tasks(
    state: tauri::State<'_, Arc<AppState>>,
    platform: Option<String>,
    limit: i32,
    offset: i32,
) -> Result<Vec<TaskRecord>, String> {
    let storage = state.storage.lock_recover();
    storage
        .list_tasks(platform.as_deref().unwrap_or(""), limit, offset)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_task(
    state: tauri::State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<(), String> {
    let storage = state.storage.lock_recover();
    storage
        .update_task_status(
            &task_id,
            crate::types::TaskStatus::Failed,
            None,
            Some("用户取消"),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_settings(
    state: tauri::State<'_, Arc<AppState>>,
    key: String,
) -> Result<Option<String>, String> {
    let storage = state.storage.lock_recover();
    storage.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_setting(
    state: tauri::State<'_, Arc<AppState>>,
    key: String,
    value: String,
) -> Result<(), String> {
    let storage = state.storage.lock_recover();
    storage.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_chrome_available(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let chrome_path = {
        let storage = state.storage.lock_recover();
        resolve_chrome_path(&storage)
    };

    let chrome_path = chrome_path.ok_or_else(|| {
        let searched = default_chrome_candidates()
            .iter()
            .map(|p| format!("  - {}", p))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "未检测到 Chrome 浏览器，请先安装 Google Chrome。\n已搜索以下路径:\n{}",
            searched
        )
    })?;

    let path = std::path::Path::new(&chrome_path);
    if !path.exists() {
        return Err(format!(
            "未检测到 Chrome 浏览器，请先安装 Google Chrome。\n当前检测路径: {}",
            chrome_path
        ));
    }

    // Windows 上 chrome.exe --version 会打开浏览器窗口，改用注册表读取版本号
    #[cfg(target_os = "windows")]
    {
        let version = get_chrome_version_from_registry().unwrap_or_default();
        if version.is_empty() {
            return Ok(format!("Chrome 已就绪 ({})", chrome_path));
        }
        return Ok(format!("Google Chrome {}", version));
    }

    // macOS / Linux 可以安全执行 --version
    #[cfg(not(target_os = "windows"))]
    {
        use std::process::Command;
        match Command::new(&chrome_path).arg("--version").output() {
            Ok(output) if output.status.success() => {
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
            }
            _ => Err(format!(
                "Chrome 文件存在但无法执行，请检查权限或重新安装。\n路径: {}",
                chrome_path
            )),
        }
    }
}

#[tauri::command]
pub async fn check_chrome_version(path: String) -> Result<String, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("路径不存在: {}", path));
    }

    // Windows 上 chrome.exe --version 会打开浏览器窗口，改用注册表
    #[cfg(target_os = "windows")]
    {
        let version = get_chrome_version_from_registry().ok_or("无法从注册表读取 Chrome 版本")?;
        return Ok(format!("Google Chrome {}", version));
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::process::Command;
        let output = Command::new(&path)
            .arg("--version")
            .output()
            .map_err(|e| format!("无法执行 Chrome: {}", e))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err("Chrome 执行失败，请检查路径".to_string())
        }
    }
}

#[tauri::command]
pub async fn get_cache_size(state: tauri::State<'_, Arc<AppState>>) -> Result<String, String> {
    let app_dir = state.app_data_dir.clone();
    let total = tokio::task::spawn_blocking(move || {
        dir_size(&app_dir.join("cache")).saturating_add(dir_size(&app_dir.join("workspace")))
    })
    .await
    .map_err(|e| format!("计算缓存大小失败: {e}"))?;
    Ok(format_bytes(total))
}

fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{} B", bytes);
    }
    let mut value = bytes as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    format!("{:.2} {}", value, UNITS[idx])
}

#[tauri::command]
pub async fn clear_cache(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    let app_dir = &state.app_data_dir;
    let cache_dir = app_dir.join("cache");
    if cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir).map_err(|e| format!("清理缓存失败: {e}"))?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| format!("重建缓存目录失败: {e}"))?;
    }
    let workspace_dir = app_dir.join("workspace");
    if workspace_dir.exists() {
        std::fs::remove_dir_all(&workspace_dir).map_err(|e| format!("清理工作区失败: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn batch_register(
    _platform: String,
    _count: i32,
    _password: String,
    _birthday: String,
) -> Result<(), String> {
    Err("批量注册功能暂未实现".to_string())
}

#[tauri::command]
pub async fn get_logs(
    state: tauri::State<'_, Arc<AppState>>,
    module: Option<String>,
    level: Option<String>,
    keyword: Option<String>,
    limit: Option<i32>,
    offset: Option<i32>,
) -> Result<Vec<crate::types::LogRecord>, String> {
    let storage = state.storage.lock_recover();
    storage
        .list_logs(
            module.as_deref().unwrap_or(""),
            level.as_deref().unwrap_or(""),
            keyword.as_deref().unwrap_or(""),
            limit.unwrap_or(200),
            offset.unwrap_or(0),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_logs(state: tauri::State<'_, Arc<AppState>>) -> Result<u64, String> {
    let storage = state.storage.lock_recover();
    storage.clear_logs().map_err(|e| e.to_string())
}

/// Windows: 从注册表读取 Chrome 版本号，避免执行 chrome.exe --version 弹出窗口
#[cfg(target_os = "windows")]
fn get_chrome_version_from_registry() -> Option<String> {
    use crate::process_ext::NoWindowExt;
    use std::process::Command;
    // reg query 本身不需要窗口，通过 CREATE_NO_WINDOW 抑制 conhost
    let output = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Google\Chrome\BLBeacon",
            "/v",
            "version",
        ])
        .no_window()
        .output()
        .ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        // 输出格式: "    version    REG_SZ    126.0.6478.127"
        return text
            .lines()
            .find(|l| l.contains("version"))
            .and_then(|l| l.split_whitespace().last())
            .map(String::from);
    }
    // 备选: 当前用户注册表
    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\SOFTWARE\Google\Chrome\BLBeacon",
            "/v",
            "version",
        ])
        .no_window()
        .output()
        .ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        return text
            .lines()
            .find(|l| l.contains("version"))
            .and_then(|l| l.split_whitespace().last())
            .map(String::from);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{dir_size, format_bytes};
    use std::fs;

    #[test]
    fn format_bytes_picks_reasonable_unit() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.00 MB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.00 GB");
    }

    #[test]
    fn dir_size_returns_zero_for_missing_path() {
        let missing = std::path::Path::new("/nonexistent/frame64/cache/xxxx");
        assert_eq!(dir_size(missing), 0);
    }

    #[test]
    fn dir_size_sums_nested_files() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();
        fs::write(root.join("a.bin"), vec![0u8; 100]).unwrap();
        let sub = root.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.bin"), vec![0u8; 250]).unwrap();
        fs::write(sub.join("c.bin"), vec![0u8; 50]).unwrap();
        assert_eq!(dir_size(root), 400);
    }
}
