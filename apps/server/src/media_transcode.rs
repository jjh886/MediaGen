use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

pub(crate) async fn run(source: Vec<u8>, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = Command::new(mediagen_core::resolve_ffmpeg_binary());
    command.args(args);
    run_to_file(command, Some(source), Duration::from_secs(60)).await
}

pub(crate) async fn run_to_file(
    mut command: Command,
    source: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let output = tempfile::NamedTempFile::new().map_err(|_| "无法创建媒体输出文件")?;
    let stdout = output.reopen().map_err(|_| "无法写入媒体输出文件")?;
    let mut child = command
        .kill_on_drop(true)
        .stdin(if source.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(stdout)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "媒体处理需要可用的 FFmpeg")?;
    let execution = async {
        if let Some(data) = source {
            let mut input = child.stdin.take().ok_or("媒体输入不可用")?;
            input.write_all(&data).await.map_err(|_| "媒体输入失败")?;
        }
        if !child.wait().await.map_err(|_| "媒体进程失败")?.success() {
            return Err("媒体处理失败");
        }
        let bytes = tokio::fs::read(output.path())
            .await
            .map_err(|_| "读取媒体输出失败")?;
        if bytes.is_empty() {
            return Err("媒体输出为空");
        }
        Ok(bytes)
    };
    tokio::time::timeout(timeout, execution)
        .await
        .map_err(|_| "媒体处理超时")?
        .map_err(ToString::to_string)
}
