use std::io::Write;
use std::path::Path;

pub fn append_startup_log(path: &Path, message: &str) {
    let _ = std::fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")));
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}

pub fn show_error(message: &str) {
    eprintln!("{message}");

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display alert \"MediaGen 启动失败\" message \"{}\"",
                    escape(message)
                ),
            ])
            .status();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Add-Type -AssemblyName PresentationFramework;[System.Windows.MessageBox]::Show('{}','MediaGen 启动失败')",
                    escape(message)
                ),
            ])
            .status();
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn escape(message: &str) -> String {
    message.replace('\\', "\\\\").replace('\'', "\\'")
}
