use std::process::Command;

pub fn open_browser(url: &str) -> Result<(), String> {
    let status = browser_command(url)
        .status()
        .map_err(|e| format!("打开浏览器失败: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("打开浏览器失败，地址：{url}"))
    }
}

fn browser_command(url: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        cmd
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        cmd
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        cmd
    }
}
