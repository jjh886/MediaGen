use serde_json::{json, Value};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

/// 仅供可信插件调用；使用参数数组，禁止 shell 拼接，限制时间和输出量。
pub(crate) async fn run(params: &Value) -> Result<Value, String> {
    let program = params["command"].as_str().ok_or("缺少 command")?;
    if !Path::new(program).is_absolute() || !Path::new(program).is_file() {
        return Err("执行文件必须是存在的绝对路径".into());
    }
    let args = params["args"].as_array().ok_or("args 必须是数组")?;
    if args.len() > 64 {
        return Err("参数数量过多".into());
    }
    let args = args
        .iter()
        .map(|arg| {
            arg.as_str()
                .filter(|value| value.len() <= 16384)
                .ok_or("进程参数无效")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .kill_on_drop(true)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| "子进程启动失败")?;
    let stdout = child.stdout.take().ok_or("缺少 stdout")?;
    let stderr = child.stderr.take().ok_or("缺少 stderr")?;
    let execution = async {
        let output = async {
            let mut bytes = Vec::new();
            stdout
                .take(65537)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "读取 stdout 失败")?;
            if bytes.len() > 65536 {
                return Err("stdout 超出限制");
            }
            Ok(bytes)
        };
        let errors = async {
            let mut bytes = Vec::new();
            stderr
                .take(65537)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "读取 stderr 失败")?;
            if bytes.len() > 65536 {
                return Err("stderr 超出限制");
            }
            Ok(bytes)
        };
        let status = async { child.wait().await.map_err(|_| "等待子进程失败") };
        let (stdout, stderr, status) = tokio::try_join!(output, errors, status)?;
        Ok::<_, &str>(
            json!({"ok":status.success(),"status":status.code(),"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr)}),
        )
    };
    tokio::time::timeout(
        Duration::from_millis(
            params["timeout_ms"]
                .as_u64()
                .unwrap_or(10000)
                .clamp(1, 60000),
        ),
        execution,
    )
    .await
    .map_err(|_| "子进程执行超时".to_string())?
    .map_err(ToString::to_string)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn arguments_are_literal_and_environment_is_isolated() {
        let result = run(&json!({"command":"/bin/echo","args":["$(must-not-run)"]}))
            .await
            .unwrap();
        assert_eq!(result["stdout"], "$(must-not-run)\n");
        let result = run(&json!({"command":"/usr/bin/env","args":[]}))
            .await
            .unwrap();
        assert_eq!(result["stdout"], "PATH=/usr/bin:/bin\n");
        let result = run(&json!({"command":"/usr/bin/false","args":[]}))
            .await
            .unwrap();
        assert_eq!(result["ok"], false);
    }
    #[tokio::test]
    async fn process_limits_reject_invalid_arguments_timeouts_and_unbounded_output() {
        for input in [
            json!({}),
            json!({"command":"echo","args":[]}),
            json!({"command":"/bin/echo","args":[1]}),
            json!({"command":"/bin/echo","args":vec!["a";65]}),
        ] {
            assert!(run(&input).await.is_err());
        }
        assert!(
            run(&json!({"command":"/bin/sleep","args":["2"],"timeout_ms":10}))
                .await
                .unwrap_err()
                .contains("超时")
        );
        assert!(
            run(&json!({"command":"/usr/bin/yes","args":[],"timeout_ms":1000}))
                .await
                .unwrap_err()
                .contains("超出限制")
        );
    }
}
