use base64::prelude::{Engine as _, BASE64_STANDARD};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub(crate) async fn transcode(params: &Value) -> Result<Value, String> {
    let format = params["format"].as_str().ok_or("缺少音频格式")?;
    let (container, codec, mime) = match format {
        "wav" => ("wav", "pcm_s16le", "audio/wav"),
        "pcm" => ("s16le", "pcm_s16le", "audio/pcm"),
        "flac" => ("flac", "flac", "audio/flac"),
        _ => return Err("不支持的转码格式".into()),
    };
    let source = BASE64_STANDARD
        .decode(params["base64"].as_str().ok_or("缺少音频数据")?)
        .map_err(|_| "音频编码无效")?;
    if source.is_empty() || source.len() > 50 * 1024 * 1024 {
        return Err("音频数据大小无效".into());
    }
    let binary = std::env::var("MEDIAGEN_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
    let mut child = Command::new(binary)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "pipe",
            "-i",
            "pipe:0",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "24000",
            "-c:a",
            codec,
            "-f",
            container,
            "pipe:1",
        ])
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "音频转码需要可用的 FFmpeg")?;
    let mut input = child.stdin.take().ok_or("音频转码输入不可用")?;
    let output = child.stdout.take().ok_or("音频转码输出不可用")?;
    let execution = async {
        let writer = async {
            input
                .write_all(&source)
                .await
                .map_err(|_| "音频转码输入失败")?;
            drop(input);
            Ok::<_, &str>(())
        };
        let reader = async {
            let mut bytes = Vec::new();
            output
                .take(50 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "音频转码输出失败")?;
            if bytes.len() > 50 * 1024 * 1024 {
                return Err("音频转码输出过大");
            }
            Ok(bytes)
        };
        let wait = async { child.wait().await.map_err(|_| "音频转码进程失败") };
        let (_, bytes, status) = tokio::try_join!(writer, reader, wait)?;
        if !status.success() || bytes.is_empty() {
            return Err("音频转码失败");
        }
        Ok(json!({"base64":BASE64_STANDARD.encode(bytes),"format":format,"mime_type":mime}))
    };
    tokio::time::timeout(Duration::from_secs(60), execution)
        .await
        .map_err(|_| "音频转码超时".to_string())?
        .map_err(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn invalid_transcoding_input_is_rejected_before_process() {
        for input in [
            json!({}),
            json!({"format":"mp3"}),
            json!({"format":"wav","base64":"invalid!"}),
            json!({"format":"pcm","base64":""}),
        ] {
            assert!(transcode(&input).await.is_err());
        }
    }
    #[tokio::test]
    async fn real_transcoding_produces_requested_containers() {
        if Command::new(std::env::var("MEDIAGEN_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()))
            .arg("-version")
            .output()
            .await
            .is_err()
        {
            eprintln!("本机未安装 FFmpeg，跳过真实容器转码测试");
            return;
        }
        let samples = 480u32;
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&24000u32.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        wav.resize(44 + samples as usize * 2, 0);
        for (format, signature) in [
            ("flac", b"fLaC".as_slice()),
            ("wav", b"RIFF".as_slice()),
            ("pcm", b"\0\0".as_slice()),
        ] {
            let result = transcode(&json!({"format":format,"base64":BASE64_STANDARD.encode(&wav)}))
                .await
                .unwrap();
            let bytes = BASE64_STANDARD
                .decode(result["base64"].as_str().unwrap())
                .unwrap();
            assert!(bytes.starts_with(signature));
            if format == "pcm" {
                assert_eq!(bytes.len(), samples as usize * 2);
            }
        }
    }
}
