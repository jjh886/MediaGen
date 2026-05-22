use serde_json::json;
use std::process::Stdio;

use super::cookie::prepare_user_data_dir;
use crate::error::AppError;
use crate::process_ext::NoWindowExt;
use crate::types::StepResult;

impl super::BrowserEngine {
    /// browser.launch_external: 用真实 Chrome 进程打开 URL，不建立 CDP 控制连接。
    ///
    /// 适合 OAuth/人工授权这类由用户在浏览器里完成、插件只等待本地回调的流程。
    /// 平台差异由插件传入 url/profile_dir/chrome_path/extra_args，Rust 只负责启动进程。
    pub(crate) async fn launch_external(
        &self,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| AppError::BrowserError("browser.launch_external 缺少 url".into()))?;
        let profile_dir = params["profile_dir"].as_str().unwrap_or("").trim();
        let chrome_path = params["chrome_path"].as_str().unwrap_or("").trim();
        if chrome_path.is_empty() {
            return Err(AppError::BrowserError(
                "browser.launch_external 缺少 chrome_path，请先在设置中配置 Chrome 路径".into(),
            ));
        }
        if !std::path::Path::new(chrome_path).exists() {
            return Err(AppError::BrowserError(format!(
                "Chrome 路径不存在: {chrome_path}"
            )));
        }
        if params["force_restart"].as_bool().unwrap_or(false) && !profile_dir.is_empty() {
            self.force_release_profile(profile_dir);
        }
        prepare_user_data_dir(profile_dir);

        let args = external_chrome_args(url, profile_dir, params);
        let mut command = std::process::Command::new(chrome_path);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .no_window();
        let child = command
            .spawn()
            .map_err(|e| AppError::BrowserError(format!("启动外部 Chrome 失败: {e}")))?;
        let pid = child.id();
        // Chrome 主进程会自行常驻；这里不等待，避免阻塞授权流程。
        drop(child);

        Ok(StepResult {
            success: true,
            data: json!({
                "pid": pid,
                "profile_dir": profile_dir,
                "url": url,
                "launched": true,
            }),
            error: None,
        })
    }
}

fn external_chrome_args(url: &str, profile_dir: &str, params: &serde_json::Value) -> Vec<String> {
    let mut args = Vec::new();
    if !profile_dir.is_empty() {
        args.push(format!("--user-data-dir={profile_dir}"));
    }
    args.extend([
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-session-crashed-bubble".to_string(),
    ]);
    if cfg!(target_os = "windows") {
        args.push("--disable-gpu".to_string());
    }
    if let Some(extra_args) = params["extra_args"].as_array() {
        args.extend(
            extra_args
                .iter()
                .filter_map(|v| v.as_str().map(String::from)),
        );
    }
    args.push("--new-window".to_string());
    args.push(url.to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_chrome_args_use_supplied_profile_and_url() {
        let args = external_chrome_args(
            "https://example.com/auth",
            "/tmp/profile",
            &json!({ "extra_args": ["--lang=zh-CN"] }),
        );

        assert!(args.contains(&"--user-data-dir=/tmp/profile".to_string()));
        assert!(args.contains(&"--lang=zh-CN".to_string()));
        assert_eq!(args.iter().rev().nth(1).unwrap(), "--new-window");
        assert_eq!(args.last().unwrap(), "https://example.com/auth");
    }
}
