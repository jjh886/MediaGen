use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::{json, Value};

pub fn kill_port(params: &Value) -> Result<Value, String> {
    let port = required_port(params)?;
    let current_pid = std::process::id();
    let targets = listener_pids_on_port(port)
        .into_iter()
        .filter(|pid| *pid != current_pid)
        .collect::<Vec<_>>();

    for pid in &targets {
        terminate_pid(*pid);
    }
    wait_for_port_release(port);

    Ok(json!({
        "port": port,
        "killed_pids": targets,
    }))
}

fn required_port(params: &Value) -> Result<u16, String> {
    let raw = params["port"]
        .as_u64()
        .ok_or_else(|| "process.kill_port 缺少 port 参数".to_string())?;
    u16::try_from(raw)
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| format!("process.kill_port 端口无效: {raw}"))
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
}
