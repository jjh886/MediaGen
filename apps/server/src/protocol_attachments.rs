use crate::{server_plugin_host::ServerPluginHost, ServerState};
use actix_web::web;
use base64::prelude::{Engine as _, BASE64_STANDARD};
use mediagen_plugin_runtime::PluginHost;
use serde_json::{json, Value};

const MAX_BYTES: usize = 50 * 1024 * 1024;

fn supports(caps: &Value, kind: &str) -> bool {
    caps.get("input_modalities")
        .or_else(|| caps.get("input_types"))
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|v| v == kind))
}

pub(crate) fn decode(part: &Value) -> Result<Option<(String, Vec<u8>)>, String> {
    let (data, fallback) = match part["type"].as_str() {
        Some("input_file") => {
            if let Some(url) = part["file_url"].as_str() {
                let parsed = reqwest::Url::parse(url).map_err(|_| "附件 URL 无效")?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    return Err("附件 URL 仅支持 HTTP/HTTPS".into());
                }
                // 支持远程文件的平台由上游处理 URL；本机不读取用户指定路径。
                return Ok(None);
            }
            let Some(data) = part["file_data"].as_str() else {
                return Err("附件缺少 file_data".into());
            };
            (
                data.to_string(),
                part["mime_type"]
                    .as_str()
                    .unwrap_or_else(|| {
                        crate::file_api::mime_from_filename(part["filename"].as_str().unwrap_or(""))
                    })
                    .to_string(),
            )
        }
        Some("input_audio") => {
            let format = part["input_audio"]["format"]
                .as_str()
                .ok_or("音频缺少 format")?;
            if !matches!(format, "wav" | "mp3") {
                return Err("音频仅支持 wav 或 mp3".into());
            }
            (
                part["input_audio"]["data"]
                    .as_str()
                    .ok_or("音频缺少数据")?
                    .to_string(),
                if format == "wav" {
                    "audio/wav"
                } else {
                    "audio/mpeg"
                }
                .into(),
            )
        }
        _ => return Ok(None),
    };
    let (mime, encoded) = if let Some(data) = data.strip_prefix("data:") {
        data.split_once(";base64,").ok_or("附件 data URL 无效")?
    } else {
        (fallback.as_str(), data.as_str())
    };
    if encoded.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err("附件超过 50 MiB".into());
    }
    let bytes = BASE64_STANDARD
        .decode(encoded)
        .map_err(|_| "附件 Base64 编码无效")?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("附件为空或超过 50 MiB".into());
    }
    Ok(Some((mime.to_ascii_lowercase(), bytes)))
}

/// 平台无关的媒体适配；能力以模型声明为准，视频降级必须明确标注抽帧。
pub(crate) async fn prepare(
    state: &web::Data<ServerState>,
    key: &str,
    request: &mut Value,
    caps: &Value,
) -> Result<(), String> {
    let Some(input) = request["input"].as_array_mut() else {
        return Ok(());
    };
    let mut cache = std::collections::HashMap::<String, Vec<Value>>::new();
    let mut total = 0;
    for item in input {
        let Some(parts) = item.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut result = Vec::new();
        for part in parts.iter() {
            let Some((mime, bytes)) = decode(part)? else {
                result.push(part.clone());
                continue;
            };
            let name = part["filename"].as_str().unwrap_or("attachment");
            let mut identity = sha2::Sha256::new();
            use sha2::Digest;
            identity.update(&bytes);
            identity.update(mime.as_bytes());
            identity.update(name.as_bytes());
            let id = format!("{:x}", identity.finalize());
            let prepared = if let Some(saved) = cache.get(&id) {
                saved.clone()
            } else {
                let prepared = prepare_one(state, key, caps, part, &mime, &bytes).await?;
                cache.insert(id, prepared.clone());
                prepared
            };
            total += serde_json::to_vec(&prepared)
                .map_err(|_| "附件编码失败")?
                .len();
            if total > 64 * 1024 * 1024 {
                return Err("本轮附件合计超过 64 MiB".into());
            }
            result.extend(prepared);
        }
        *parts = result;
    }
    Ok(())
}

async fn prepare_one(
    state: &web::Data<ServerState>,
    key: &str,
    caps: &Value,
    part: &Value,
    mime: &str,
    bytes: &[u8],
) -> Result<Vec<Value>, String> {
    if mime.starts_with("image/") {
        if !supports(caps, "image") {
            return Err("当前模型不支持图片输入".into());
        }
        return Ok(vec![
            json!({"type":"input_image","image_url":format!("data:{mime};base64,{}",BASE64_STANDARD.encode(bytes))}),
        ]);
    }
    if mime.starts_with("audio/") {
        return Ok(vec![
            audio_part(state, key, caps, bytes.to_vec(), mime).await?,
        ]);
    }
    if !mime.starts_with("video/") {
        if !supports(caps, "file") {
            return Err("当前模型不支持文件输入".into());
        }
        if let Some(extensions) = caps["file_extensions"].as_array() {
            let name = part["filename"].as_str().unwrap_or("").to_lowercase();
            if !extensions
                .iter()
                .filter_map(Value::as_str)
                .any(|ext| !ext.is_empty() && name.ends_with(&format!(".{ext}")))
            {
                return Err("文件后缀不在模型允许范围内".into());
            }
        }
        return Ok(vec![part.clone()]);
    }
    let work = tempfile::tempdir().map_err(|_| "创建媒体临时目录失败")?;
    let path = work.path().join("video");
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|_| "媒体附件写入失败")?;
    let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
    let info = host.call("file.inspect_media", &json!({"path":path})).await;
    if !info.success {
        return Err(info.error.unwrap_or("媒体信息检查失败".into()));
    }
    let duration = info.data["duration"].as_f64().ok_or("视频时长无效")?;
    let times = frame_times(duration)?;
    if info.data["has_video"] != true {
        return Err("附件没有视频画面".into());
    }
    let mut output = if supports(caps, "video") && bytes.len() <= 16 * 1024 * 1024 {
        vec![part.clone()]
    } else {
        if !supports(caps, "image") {
            return Err("当前模型不支持视频或图片输入".into());
        }
        let mut parts = vec![
            json!({"type":"input_text","text":format!("视频附件，时长 {duration:.2} 秒。以下为均匀采样画面，不代表已读取全部帧；精细动作需进一步提帧。")}),
        ];
        for second in times {
            let frame = crate::media_input::ffmpeg_file(
                &path,
                &[
                    "-ss",
                    &format!("{second:.3}"),
                    "-frames:v",
                    "1",
                    "-vf",
                    "scale=1024:1024:force_original_aspect_ratio=decrease",
                    "-f",
                    "image2pipe",
                    "-vcodec",
                    "mjpeg",
                ],
            )
            .await?;
            parts.push(json!({"type":"input_text","text":format!("视频时间：{second:.3} 秒")}));
            parts.push(json!({"type":"input_image","image_url":format!("data:image/jpeg;base64,{}",BASE64_STANDARD.encode(frame))}));
        }
        parts
    };
    if info.data["has_audio"] == true {
        let audio = crate::media_input::ffmpeg_file(
            &path,
            &[
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-c:a",
                "pcm_s16le",
                "-f",
                "wav",
            ],
        )
        .await?;
        output.push(audio_part(state, key, caps, audio, "audio/wav").await?);
    }
    Ok(output)
}

fn frame_times(duration: f64) -> Result<Vec<f64>, String> {
    if !duration.is_finite() || duration <= 0.0 || duration > 3600.0 {
        return Err("视频理解支持时长不超过 1 小时的有效视频".into());
    }
    let count = (duration.ceil() as usize).clamp(1, 8);
    Ok((0..count)
        .map(|i| duration * i as f64 / count as f64)
        .collect())
}

async fn audio_part(
    state: &web::Data<ServerState>,
    key: &str,
    caps: &Value,
    bytes: Vec<u8>,
    mime: &str,
) -> Result<Value, String> {
    let (data, format) = if matches!(mime, "audio/mpeg" | "audio/mp3") {
        (BASE64_STANDARD.encode(bytes), "mp3")
    } else if matches!(mime, "audio/wav" | "audio/x-wav") {
        (BASE64_STANDARD.encode(bytes), "wav")
    } else {
        (
            crate::audio_transcode::transcode(
                &json!({"base64":BASE64_STANDARD.encode(bytes),"format":"wav"}),
            )
            .await?["base64"]
                .as_str()
                .ok_or("音频转换失败")?
                .to_string(),
            "wav",
        )
    };
    if data.len() > (20usize * 1024 * 1024).div_ceil(3) * 4 {
        return Err("音频输入超过 20 MiB，请缩短音频".into());
    }
    if supports(caps, "audio") {
        return Ok(json!({"type":"input_audio","input_audio":{"data":data,"format":format}}));
    }
    let text = crate::media_input::transcribe(state, key, &data, format).await?;
    Ok(json!({"type":"input_text","text":format!("附件音轨转写（并非原始声音输入）：\n{text}")}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn real_video_retains_native_bytes_or_produces_distinct_sampled_frames() {
        let binary = std::env::var("MEDIAGEN_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
        if tokio::process::Command::new(&binary)
            .arg("-version")
            .output()
            .await
            .is_err()
        {
            return;
        }
        let work = tempfile::tempdir().unwrap();
        let path = work.path().join("sample.mp4");
        let result = tokio::process::Command::new(binary)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=128x96:rate=4",
                "-t",
                "2",
                "-c:v",
                "mpeg4",
                "-an",
            ])
            .arg(&path)
            .output()
            .await
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let bytes = tokio::fs::read(&path).await.unwrap();
        let part = json!({"type":"input_file","filename":"sample.mp4","file_data":format!("data:video/mp4;base64,{}",BASE64_STANDARD.encode(&bytes))});
        let state = crate::protocol_api_tests::state();
        let native = prepare_one(
            &state,
            "key-a",
            &json!({"input_modalities":["text","image","video","file"]}),
            &part,
            "video/mp4",
            &bytes,
        )
        .await
        .unwrap();
        assert_eq!(native, vec![part.clone()]);
        let sampled = prepare_one(
            &state,
            "key-a",
            &json!({"input_modalities":["text","image","file"]}),
            &part,
            "video/mp4",
            &bytes,
        )
        .await
        .unwrap();
        let frames = sampled
            .iter()
            .filter(|p| p["type"] == "input_image")
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 2);
        assert_ne!(frames[0]["image_url"], frames[1]["image_url"]);
        assert!(sampled[0]["text"].as_str().unwrap().contains("均匀采样"));
        for frame in frames {
            let encoded = frame["image_url"]
                .as_str()
                .unwrap()
                .split_once(',')
                .unwrap()
                .1;
            assert_eq!(
                image::guess_format(&BASE64_STANDARD.decode(encoded).unwrap()).unwrap(),
                image::ImageFormat::Jpeg
            );
        }
    }

    #[test]
    fn validates_media_bytes_and_samples_full_duration() {
        assert!(
            decode(&json!({"type":"input_file","file_url":"https://example.com/report.pdf"}))
                .unwrap()
                .is_none()
        );
        assert!(decode(&json!({"type":"input_file","file_url":"file:///private/data"})).is_err());
        assert_eq!(
            decode(&json!({"type":"input_file","filename":"clip.mp4","file_data":"YWJj"}))
                .unwrap()
                .unwrap()
                .0,
            "video/mp4"
        );
        assert!(
            decode(&json!({"type":"input_audio","input_audio":{"format":"mp3","data":"!"}}))
                .is_err()
        );
        assert!(decode(&json!({"type":"input_file","file_data":""})).is_err());
        assert_eq!(
            frame_times(8.0).unwrap(),
            vec![0., 1., 2., 3., 4., 5., 6., 7.]
        );
        assert!(frame_times(f64::NAN).is_err());
        assert!(frame_times(3601.).is_err());
    }
}
