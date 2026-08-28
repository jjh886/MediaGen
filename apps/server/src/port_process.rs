use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::{json, Value};

pub fn spawn_process(params: &Value) -> Result<Value, String> {
    let command = params["command"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "process.spawn 缺少 command".to_string())?;
    let command_path = std::path::Path::new(command);
    if !command_path.is_absolute() || !command_path.is_file() {
        return Err(format!(
            "process.spawn command 必须是存在的绝对文件路径: {command}"
        ));
    }
    let args = params["args"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|value| value.len() <= 4096)
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| "process.spawn args 只能包含短字符串".to_string())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    if args.len() > 64 {
        return Err("process.spawn args 不能超过 64 项".to_string());
    }
    let mut process = std::process::Command::new(command);
    process
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(cwd) = params["cwd"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    {
        let cwd_path = std::path::Path::new(cwd);
        if !cwd_path.is_absolute() || !cwd_path.is_dir() {
            return Err(format!("process.spawn cwd 必须是存在的绝对目录: {cwd}"));
        }
        process.current_dir(cwd_path);
    }
    let child = process
        .spawn()
        .map_err(|error| format!("process.spawn 启动失败: {error}"))?;
    Ok(json!({ "pid": child.id(), "command": command, "args": args }))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_rejects_relative_or_missing_commands() {
        assert!(spawn_process(&json!({ "command": "relative-command" })).is_err());
        assert!(spawn_process(&json!({ "command": "/definitely/missing" })).is_err());
    }
}
