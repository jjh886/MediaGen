use std::sync::Arc;

/// 对一组 tab 依次发 CDP `Page.Close`（触发 beforeunload），让 Chrome 自己进入关闭流程。
pub(crate) fn close_tabs_with_unload(tabs: &[Arc<headless_chrome::Tab>]) {
    for tab in tabs {
        let _ = tab.close_with_unload();
    }
}

/// 核心：向 PID 发"优雅退出"信号并同步轮询等待进程自然退出。
///
/// 只要进程在 `timeout` 内自行退出就返回 true；否则返回 false 并打一条 warn（由调用方决定
/// 是走 Drop 的 SIGKILL 兜底，还是显式 force kill）。
///
/// 这个函数是同步的，同时服务三条路径：
/// - 登录 / 重新登录：本身在 `spawn_blocking` 里，直接调用
/// - 人工验证：同上
/// - 任务结束 (async close)：在 tokio `spawn_blocking` 里调用，避免阻塞运行时
pub(crate) fn graceful_terminate_and_wait(
    pid: u32,
    timeout: std::time::Duration,
    label: &str,
) -> bool {
    terminate_chrome_gracefully(pid);
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !pid_is_alive(pid) {
            tracing::info!(pid, label, "Chrome 已优雅退出");
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    tracing::warn!(
        pid,
        label,
        timeout_secs = timeout.as_secs(),
        "Chrome 超时未优雅退出，将由 Browser::Drop 兜底（最终 SIGKILL）"
    );
    false
}

/// 完整地优雅关闭一个**独立持有**的 Browser（未被 BrowserEngine 注册管理的场景：
/// 登录 / 重新登录 / 人工验证等）。统一出口，保证这三条路径的关闭语义一致。
///
/// 调用方只管把拥有所有权的 `browser` + 主 `tab` 交过来，函数负责：
/// 1. CDP `Page.Close`（触发 beforeunload）
/// 2. SIGTERM / taskkill（graceful）
/// 3. 轮询 5 秒等进程自然退出
/// 4. Drop browser：若进程已退这是纯清理；若未退 Drop 内部会做最后 SIGKILL 兜底
pub(crate) fn graceful_shutdown_owned_browser(
    browser: headless_chrome::Browser,
    tab: Arc<headless_chrome::Tab>,
    label: &str,
) {
    let pid = browser.get_process_id();
    close_tabs_with_unload(&[tab.clone()]);
    drop(tab);
    if let Some(pid) = pid {
        graceful_terminate_and_wait(pid, std::time::Duration::from_secs(5), label);
    }
    drop(browser);
    tracing::info!(label, "浏览器已关闭（优雅 + Drop 兜底）");
}

/// 轻量级检测进程是否仍存活，不产生副作用。用于等待 Chrome 优雅退出。
///
/// Unix：必须排除 zombie（defunct）状态。原先用 `kill -0` 只检查进程表存在性，
/// 而 `headless_chrome::TemporaryProcess` 仅在 Drop 时才 wait() 子进程，导致
/// Chrome 虽已响应 SIGTERM 退出、但在我们 5 秒轮询窗口内一直表现为"zombie"，
/// `kill -0` 仍返回成功 → 误判为存活 → 每次关闭都超时打出"最终 SIGKILL"警告。
/// 改用 `ps -o state=`：空输出=进程已消失；以 'Z' 开头=僵尸（已退出未 reap）；
/// 其它为真正存活。
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        match std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "state="])
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(out) => {
                let state = std::str::from_utf8(&out.stdout).unwrap_or("").trim();
                !state.is_empty() && !state.starts_with('Z')
            }
            Err(_) => false,
        }
    }
    #[cfg(windows)]
    {
        // Windows 没有 Unix 意义上的 zombie——进程退出后立刻从 tasklist 里消失，
        // 父进程持有的 HANDLE 只用来取退出码，不影响 tasklist 枚举。
        // 用 CSV 格式精确匹配 PID 字段，避免内存列（如 "50,000 K"）里的数字
        // 恰好以目标 PID 起头时被子串匹配误判为存活。
        use crate::process_ext::NoWindowExt;
        match std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH", "/FO", "CSV"])
            .no_window()
            .output()
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let target = pid.to_string();
                stdout.lines().any(|line| {
                    line.split(',')
                        .nth(1)
                        .map(|f| f.trim().trim_matches('"') == target)
                        .unwrap_or(false)
                })
            }
            Err(_) => false,
        }
    }
}

/// 向指定 PID 发送"优雅退出"信号（等价于用户菜单里的"退出"）：
/// - Unix：SIGTERM，Chrome 会运行 atexit、落盘 session/cookies、等 render 子进程收尾再退出
/// - Windows：`taskkill /PID`（不带 `/F`），发送 WM_CLOSE 让 Chrome 主窗口正常关闭
///
/// 与 `kill_chrome_by_profile` 不同：不是 SIGKILL/TerminateProcess，不会丢失状态。
fn terminate_chrome_gracefully(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .output();
    }
    #[cfg(windows)]
    {
        use crate::process_ext::NoWindowExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .no_window()
            .output();
    }
}

/// 杀掉使用指定 profile_dir 的残留 Chrome 进程。
pub(crate) fn kill_chrome_by_profile(profile_dir: &str) {
    if profile_dir.is_empty() {
        return;
    }
    tracing::info!(profile_dir, "清理残留 Chrome 进程");
    #[cfg(unix)]
    {
        let pattern = format!("--user-data-dir={profile_dir}");
        terminate_profile_processes_unix(&pattern, false);
        for _ in 0..5 {
            if !has_profile_processes_unix(&pattern) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
        if has_profile_processes_unix(&pattern) {
            tracing::warn!(profile_dir, "发现残留 Chrome 进程，升级为 SIGKILL");
            terminate_profile_processes_unix(&pattern, true);
            for _ in 0..5 {
                if !has_profile_processes_unix(&pattern) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        }
    }
    #[cfg(windows)]
    {
        // Windows 上 taskkill 的 COMMANDLINE 过滤不支持通配符，
        // 改用 PowerShell + Get-CimInstance 静默查找 profile，再由 Rust 调用
        // no_window 的 taskkill /T /F 结束整棵 Chrome 进程树。
        let escaped = profile_dir.replace('\'', "''");
        let ps_script = format!(
            "Get-CimInstance Win32_Process -Filter \"Name='chrome.exe'\" | \
             Where-Object {{ $_.CommandLine -match [regex]::Escape('--user-data-dir={escaped}') }} | \
             Select-Object -ExpandProperty ProcessId"
        );
        use crate::process_ext::NoWindowExt;
        match std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_script])
            .no_window()
            .output()
        {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    tracing::debug!(stderr = %stderr, "PowerShell 清理进程输出");
                }
                for pid in parse_pid_lines(&String::from_utf8_lossy(&output.stdout)) {
                    force_kill_pid_tree_windows(pid);
                }
            }
            Err(e) => tracing::warn!(error = %e, "PowerShell 执行失败"),
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(1500));
    let dir = std::path::Path::new(profile_dir);
    for lock in &["SingletonLock", "SingletonSocket", "SingletonCookie"] {
        let _ = std::fs::remove_file(dir.join(lock));
    }
}

#[cfg(windows)]
fn parse_pid_lines(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0)
        .collect()
}

#[cfg(windows)]
fn force_kill_pid_tree_windows(pid: u32) {
    use crate::process_ext::NoWindowExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .no_window()
        .output();
}

#[cfg(unix)]
fn has_profile_processes_unix(pattern: &str) -> bool {
    match std::process::Command::new("pgrep")
        .args(["-f", "--", pattern])
        .output()
    {
        Ok(output) => output.status.success() && !output.stdout.is_empty(),
        Err(e) => {
            tracing::debug!(error = %e, pattern, "pgrep 执行失败");
            false
        }
    }
}

#[cfg(unix)]
fn terminate_profile_processes_unix(pattern: &str, force: bool) {
    let signal = if force { "-KILL" } else { "-TERM" };
    match std::process::Command::new("pkill")
        .args([signal, "-f", "--", pattern])
        .output()
    {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::debug!(signal, pattern, stderr = %stderr, "pkill 清理进程输出");
            }
        }
        Err(e) => tracing::warn!(error = %e, signal, pattern, "pkill 执行失败"),
    }
}
