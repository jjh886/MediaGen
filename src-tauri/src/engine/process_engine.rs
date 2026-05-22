use std::collections::BTreeSet;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::types::StepResult;

#[cfg(windows)]
use crate::process_ext::NoWindowExt;

pub struct ProcessEngine;

impl ProcessEngine {
    pub fn new() -> Self {
        Self
    }

    async fn kill_port(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let port = required_port(params)?;
        let pids = listener_pids_on_port(port);
        let current_pid = std::process::id();
        let targets = pids
            .into_iter()
            .filter(|pid| *pid != current_pid)
            .collect::<Vec<_>>();
        terminate_pids(&targets);
        wait_for_port_release(port);

        Ok(StepResult {
            success: true,
            data: json!({
                "port": port,
                "killed_pids": targets,
            }),
            error: None,
        })
    }
}

#[async_trait]
impl AtomExecutor for ProcessEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "process"
    }

    async fn execute(
        &self,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        match action {
            "process.kill_port" => self.kill_port(params).await,
            _ => Err(AppError::PlatformError(format!(
                "未知的 process 操作: {action}，可用: process.kill_port"
            ))),
        }
    }
}

fn required_port(params: &serde_json::Value) -> Result<u16, AppError> {
    let raw = params["port"]
        .as_u64()
        .ok_or_else(|| AppError::PlatformError("process.kill_port 缺少 port 参数".into()))?;
    u16::try_from(raw)
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| AppError::PlatformError(format!("process.kill_port 端口无效: {raw}")))
}

fn listener_pids_on_port(port: u16) -> Vec<u32> {
    let mut pids = BTreeSet::new();
    for pid in listener_pids_platform(port) {
        pids.insert(pid);
    }
    pids.into_iter().collect()
}

#[cfg(unix)]
fn listener_pids_platform(port: u16) -> Vec<u32> {
    let output = std::process::Command::new("lsof")
        .args(["-nP", "-t", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output();
    output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_pid_lines(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(windows)]
fn listener_pids_platform(port: u16) -> Vec<u32> {
    let script = format!(
        "Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | \
         Select-Object -ExpandProperty OwningProcess"
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .no_window()
        .output();
    output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_pid_lines(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

#[cfg(not(any(unix, windows)))]
fn listener_pids_platform(_port: u16) -> Vec<u32> {
    Vec::new()
}

fn parse_pid_lines(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0)
        .collect()
}

fn terminate_pids(pids: &[u32]) {
    for pid in pids {
        terminate_pid(*pid);
    }
}

#[cfg(unix)]
fn terminate_pid(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .output();
}

#[cfg(windows)]
fn terminate_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .no_window()
        .output();
}

#[cfg(not(any(unix, windows)))]
fn terminate_pid(_pid: u32) {}

fn wait_for_port_release(port: u16) {
    for _ in 0..20 {
        if listener_pids_on_port(port).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    force_terminate_port(port);
}

#[cfg(unix)]
fn force_terminate_port(port: u16) {
    for pid in listener_pids_on_port(port) {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output();
    }
}

#[cfg(not(unix))]
fn force_terminate_port(_port: u16) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_pid_lines_ignores_noise() {
        assert_eq!(parse_pid_lines("123\nabc\n0\n456\n"), vec![123, 456]);
    }

    #[test]
    fn required_port_rejects_zero_and_overflow() {
        assert!(required_port(&json!({ "port": 1455 })).is_ok());
        assert!(required_port(&json!({ "port": 0 })).is_err());
        assert!(required_port(&json!({ "port": 70000 })).is_err());
    }
}
