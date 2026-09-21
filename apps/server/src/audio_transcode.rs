use base64::prelude::{Engine as _, BASE64_STANDARD};
use serde_json::{json, Value};

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
    let bytes = crate::media_transcode::run(
        source,
        &[
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
        ],
    )
    .await?;
    Ok(json!({"base64":BASE64_STANDARD.encode(bytes),"format":format,"mime_type":mime}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::process::Command;
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
