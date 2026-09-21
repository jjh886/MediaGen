use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub(crate) async fn run(source: Vec<u8>, args: &[&str]) -> Result<Vec<u8>, String> {
    let binary = std::env::var("MEDIAGEN_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
    let mut child = Command::new(binary)
        .args(args)
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "媒体转码需要可用的 FFmpeg")?;
    let mut input = child.stdin.take().ok_or("媒体转码输入不可用")?;
    let output = child.stdout.take().ok_or("媒体转码输出不可用")?;
    let execution = async {
        let writer = async {
            input
                .write_all(&source)
                .await
                .map_err(|_| "媒体转码输入失败")?;
            drop(input);
            Ok::<_, &str>(())
        };
        let reader = async {
            let mut bytes = Vec::new();
            output
                .take(50 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "媒体转码输出失败")?;
            if bytes.len() > 50 * 1024 * 1024 {
                return Err("媒体转码输出过大");
            }
            Ok(bytes)
        };
        let wait = async { child.wait().await.map_err(|_| "媒体转码进程失败") };
        let (_, bytes, status) = tokio::try_join!(writer, reader, wait)?;
        if !status.success() || bytes.is_empty() {
            return Err("媒体转码失败");
        }
        Ok(bytes)
    };
    tokio::time::timeout(Duration::from_secs(60), execution)
        .await
        .map_err(|_| "媒体转码超时".to_string())?
        .map_err(ToString::to_string)
}
