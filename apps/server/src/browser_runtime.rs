use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

const CLOAK_RELEASES_API: &str =
    "https://api.github.com/repos/CloakHQ/CloakBrowser/releases?per_page=100";

#[derive(Debug, Clone, Serialize)]
pub struct BrowserRuntimeInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub installed: bool,
    pub path: String,
    pub version: String,
    pub healthy: bool,
}

pub fn detect_cloakbrowser() -> BrowserRuntimeInfo {
    let path = find_cloakbrowser_path();
    let version = path
        .as_deref()
        .and_then(read_browser_version)
        .unwrap_or_default();
    BrowserRuntimeInfo {
        id: "cloakbrowser",
        name: "CloakBrowser",
        installed: path.is_some(),
        path: path
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default(),
        healthy: !version.is_empty(),
        version,
    }
}

pub fn install_cloakbrowser() -> Result<BrowserRuntimeInfo, String> {
    let current = detect_cloakbrowser();
    if current.healthy {
        return Ok(current);
    }

    run_install_command()?;
    let next = detect_cloakbrowser();
    if next.healthy {
        Ok(next)
    } else {
        Err(
            "自动安装后仍未检测到 CloakBrowser，请检查网络或安装包是否携带 browser-runtime。"
                .to_string(),
        )
    }
}

pub fn install_steps() -> Vec<&'static str> {
    if cfg!(target_os = "windows") {
        vec![
            "优先使用安装包内置的 browser-runtime，无需用户单独安装。",
            "如果安装包没有内置，点击“自动安装/修复”下载到当前用户目录。",
            "企业离线部署可把 Windows 版 CloakBrowser 放到 browser-runtime/CloakBrowser/。",
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            "优先使用 MediaGen.app 内置的 browser-runtime，无需用户单独安装。",
            "如果安装包没有内置，点击“自动安装/修复”下载到 ~/.cloakbrowser。",
            "企业离线部署可把 macOS 版 Chromium.app 放到 browser-runtime/。",
        ]
    } else {
        vec![
            "优先使用安装包内置的 browser-runtime，无需用户单独安装。",
            "如果安装包没有内置，点击“自动安装/修复”下载到 ~/.cloakbrowser。",
            "服务器离线部署可把 Linux 版 CloakBrowser 放到 browser-runtime/cloakbrowser/。",
        ]
    }
}

pub fn find_cloakbrowser_path() -> Option<PathBuf> {
    env_candidates()
        .into_iter()
        .chain(bundled_candidates())
        .chain(system_candidates())
        .chain(cache_candidates())
        .find(|path| path.exists())
}

fn env_candidates() -> Vec<PathBuf> {
    ["CLOAK_BROWSER_PATH", "CLOAKBROWSER_BINARY_PATH"]
        .iter()
        .filter_map(|name| std::env::var_os(name).map(PathBuf::from))
        .collect()
}

fn bundled_candidates() -> Vec<PathBuf> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        return vec![
            root.join("browser-runtime/Chromium.app/Contents/MacOS/Chromium"),
            root.join("browser-runtime/CloakBrowser.app/Contents/MacOS/CloakBrowser"),
            root.join("browser-runtime/Cloak Browser.app/Contents/MacOS/CloakBrowser"),
        ];
    }
    if cfg!(target_os = "windows") {
        return vec![
            root.join("browser-runtime/CloakBrowser/chrome.exe"),
            root.join("browser-runtime/CloakBrowser/Application/cloakbrowser.exe"),
            root.join("browser-runtime/chrome.exe"),
        ];
    }
    vec![
        root.join("browser-runtime/cloakbrowser/chrome"),
        root.join("browser-runtime/cloakbrowser"),
    ]
}

fn system_candidates() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        return vec![
            "/Applications/CloakBrowser.app/Contents/MacOS/CloakBrowser".into(),
            "/Applications/Cloak Browser.app/Contents/MacOS/CloakBrowser".into(),
        ];
    }
    if cfg!(target_os = "windows") {
        return vec![
            r"C:\Program Files\CloakBrowser\Application\cloakbrowser.exe".into(),
            r"C:\Program Files (x86)\CloakBrowser\Application\cloakbrowser.exe".into(),
        ];
    }
    vec![
        "/usr/bin/cloakbrowser".into(),
        "/usr/local/bin/cloakbrowser".into(),
        "/opt/cloakbrowser/chrome".into(),
    ]
}

fn cache_candidates() -> Vec<PathBuf> {
    let cache = cloakbrowser_cache_dir();
    let mut versions = std::fs::read_dir(cache)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("chromium-"))
        })
        .collect::<Vec<_>>();
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    versions.into_iter().map(cache_binary_path).collect()
}

fn cache_binary_path(dir: PathBuf) -> PathBuf {
    if cfg!(target_os = "macos") {
        return dir.join("Chromium.app/Contents/MacOS/Chromium");
    }
    if cfg!(target_os = "windows") {
        return dir.join("chrome.exe");
    }
    dir.join("chrome")
}

fn cloakbrowser_cache_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("CLOAKBROWSER_CACHE_DIR") {
        return PathBuf::from(path);
    }
    user_home()
        .map(|home| home.join(".cloakbrowser"))
        .unwrap_or_else(|| PathBuf::from(".cloakbrowser"))
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn read_browser_version(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

fn run_install_command() -> Result<(), String> {
    let platform = platform_tag()?;
    let release = latest_release(&platform)?;
    let tag = release["tag_name"].as_str().unwrap_or("latest");
    let asset = find_asset(&release, &asset_name(&platform))?;
    let url = asset["browser_download_url"]
        .as_str()
        .ok_or_else(|| "CloakBrowser release 缺少下载地址".to_string())?;
    let work_dir = cloakbrowser_cache_dir().join("downloads").join(tag);
    let archive = work_dir.join(asset["name"].as_str().unwrap_or("cloakbrowser"));
    let extract_dir = work_dir.join("extract");
    let runtime_dir = cloakbrowser_cache_dir().join(format!("chromium-{tag}"));

    std::fs::create_dir_all(&work_dir).map_err(|err| err.to_string())?;
    download_file(url, &archive)?;
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir).map_err(|err| err.to_string())?;
    extract_archive(&archive, &extract_dir)?;
    let from = runtime_root(&extract_dir)?;
    let _ = std::fs::remove_dir_all(&runtime_dir);
    copy_dir_all(&from, &runtime_dir)?;
    chmod_runtime(&runtime_dir);
    Ok(())
}

fn run_command(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("无法运行 {program}: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{program} 安装失败: {}", stderr.trim()))
}

fn latest_release(platform: &str) -> Result<Value, String> {
    let text = download_text(CLOAK_RELEASES_API)?;
    let releases: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    releases
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item["draft"].as_bool() != Some(true)
                    && item["prerelease"].as_bool() != Some(true)
                    && !item["tag_name"].as_str().unwrap_or("").contains("-pro")
                    && find_asset(item, &asset_name(platform)).is_ok()
            })
        })
        .cloned()
        .ok_or_else(|| format!("没有找到 {platform} 的 CloakBrowser 免费版 release"))
}

fn find_asset(release: &Value, name: &str) -> Result<Value, String> {
    release["assets"]
        .as_array()
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset["name"].as_str() == Some(name))
        })
        .cloned()
        .ok_or_else(|| format!("CloakBrowser release 缺少资源: {name}"))
}

fn download_text(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "-H", "User-Agent: MediaGen-Server", url])
        .output()
        .map_err(|err| format!("无法运行 curl 下载 CloakBrowser 信息: {err}"))?;
    if output.status.success() {
        return String::from_utf8(output.stdout).map_err(|err| err.to_string());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

fn download_file(url: &str, target: &Path) -> Result<(), String> {
    let target_text = target.to_string_lossy().to_string();
    run_command(
        "curl",
        &[
            "-fL",
            "-H",
            "User-Agent: MediaGen-Server",
            "-o",
            &target_text,
            url,
        ],
    )
}

fn extract_archive(archive: &Path, target: &Path) -> Result<(), String> {
    let archive_text = archive.to_string_lossy().to_string();
    let target_text = target.to_string_lossy().to_string();
    if archive_text.ends_with(".zip") {
        if cfg!(target_os = "windows") {
            return run_command(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Expand-Archive -Force -Path '{}' -DestinationPath '{}'",
                        archive_text.replace('\'', "''"),
                        target_text.replace('\'', "''")
                    ),
                ],
            );
        }
        return run_command("unzip", &["-q", &archive_text, "-d", &target_text]);
    }
    run_command("tar", &["-xzf", &archive_text, "-C", &target_text])
}

fn runtime_root(extract_dir: &Path) -> Result<PathBuf, String> {
    for relative in [
        "Chromium.app",
        "chrome",
        "chrome.exe",
        "cloakbrowser",
        "chrome-linux",
        "chrome-win",
        "chromium",
    ] {
        let candidate = extract_dir.join(relative);
        if candidate.exists() {
            return Ok(if candidate.is_file() || relative == "Chromium.app" {
                extract_dir.to_path_buf()
            } else {
                candidate
            });
        }
    }
    Err(format!(
        "无法识别 CloakBrowser 解压目录: {}",
        extract_dir.display()
    ))
}

fn copy_dir_all(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|err| err.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target).map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

fn chmod_runtime(dir: &Path) {
    for relative in ["chrome", "Chromium.app/Contents/MacOS/Chromium"] {
        let file = dir.join(relative);
        if file.exists() {
            let file_text = file.to_string_lossy().to_string();
            let _ = Command::new("chmod").args(["755", &file_text]).output();
        }
    }
}

fn platform_tag() -> Result<String, String> {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "macos" => Ok(if arch == "aarch64" {
            "darwin-arm64"
        } else {
            "darwin-x64"
        }
        .to_string()),
        "linux" => Ok(if arch == "aarch64" {
            "linux-arm64"
        } else {
            "linux-x64"
        }
        .to_string()),
        "windows" => Ok("windows-x64".to_string()),
        other => Err(format!("不支持的 CloakBrowser 平台: {other}-{arch}")),
    }
}

fn asset_name(platform: &str) -> String {
    let ext = if platform.starts_with("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("cloakbrowser-{platform}.{ext}")
}
