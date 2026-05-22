//! 子进程扩展：Windows 下静默运行，避免弹出 cmd / conhost 窗口。
//!
//! 背景：Windows 上用 `Command::new` 启动控制台程序（taskkill/tasklist/
//! powershell/reg/ffmpeg 等）默认会附带一个可见的控制台窗口。Chrome 优雅
//! 关闭的 `pid_is_alive` 轮询会频繁触发 `tasklist`，造成屏幕上闪出大量
//! 窗口。通过设置 `CREATE_NO_WINDOW` 标志让这些短命子进程静默执行。
//! 非 Windows 平台为 no-op。

/// `CREATE_NO_WINDOW`（winbase.h），让子进程不要创建控制台窗口。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub trait NoWindowExt {
    /// Windows：附加 `CREATE_NO_WINDOW` 标志；其他平台无操作。
    fn no_window(&mut self) -> &mut Self;
}

impl NoWindowExt for std::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(CREATE_NO_WINDOW)
        }
        #[cfg(not(windows))]
        {
            self
        }
    }
}

impl NoWindowExt for tokio::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            self.creation_flags(CREATE_NO_WINDOW)
        }
        #[cfg(not(windows))]
        {
            self
        }
    }
}
