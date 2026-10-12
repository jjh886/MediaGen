use crate::ServerState;
use actix_web::web;
use serde_json::{json, Value};

pub(crate) async fn ffmpeg_file(path: &std::path::Path, args: &[&str]) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    let mut child = tokio::process::Command::new(mediagen_core::resolve_ffmpeg_binary())
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args(args)
        .arg("pipe:1")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "视频处理需要可用的 FFmpeg")?;
    let output = child.stdout.take().ok_or("媒体输出不可用")?;
    let execution = async {
        let mut bytes = Vec::new();
        output
            .take(50 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "媒体输出失败")?;
        if bytes.len() > 50 * 1024 * 1024 {
            return Err("媒体输出超过 50 MiB".to_string());
        }
        if !child.wait().await.map_err(|_| "媒体进程失败")?.success() || bytes.is_empty() {
            return Err("媒体处理失败".into());
        }
        Ok(bytes)
    };
    tokio::time::timeout(std::time::Duration::from_secs(120), execution)
        .await
        .map_err(|_| "媒体处理超时")?
}

pub(crate) async fn transcribe(
    state: &web::Data<ServerState>,
    key: &str,
    data: &str,
    format: &str,
) -> Result<String, String> {
    let cache = transcript_path(state, key, data, format);
    if let Ok(encrypted) = tokio::fs::read_to_string(&cache).await {
        if let Ok(text) = state.credential_cipher.decrypt(&encrypted) {
            if !text.trim().is_empty() {
                return Ok(text);
            }
        }
    }
    for (manifest, model) in state.plugin_loader.all_models() {
        if model.task_kind != mediagen_core::TaskKind::AudioTranscription
            || !manifest.flows.contains_key("transcribe_audio")
            || crate::model_api::resolve(state, &model.model_id, key).is_none()
        {
            continue;
        }
        let task_id = format!("attachment-{}", uuid::Uuid::new_v4());
        let request = json!({"model":model.model_id,"file":{"base64":data,"filename":format!("audio.{format}"),"mime_type":if format=="mp3" {"audio/mpeg"} else {"audio/wav"}},"response_format":"json"});
        let Ok((mut account, _lease)) = crate::protocol_task::reserve(
            state,
            manifest,
            &model.model_id,
            "audio_transcription",
            key,
            &task_id,
            Some(&request),
        ) else {
            continue;
        };
        let result = crate::protocol_task::run_with_failover(
            state,
            manifest,
            &mut account,
            "transcribe_audio",
            request,
            None,
            &task_id,
            Value::Null,
            key,
        )
        .await;
        match result {
            Ok(value) => {
                let text = value["text"]
                    .as_str()
                    .filter(|v| !v.trim().is_empty())
                    .ok_or("音轨未返回有效转写文本")?;
                state.store.mark_account_usage(&account.id);
                state
                    .store
                    .complete_task(&task_id, json!({"status":"completed"}));
                if let Ok(encrypted) = state.credential_cipher.encrypt(text) {
                    let _ = crate::account_login_credentials::write_private_file(
                        &cache,
                        encrypted.as_bytes(),
                    )
                    .await;
                }
                return Ok(text.to_string());
            }
            Err(_) => {
                state.store.fail_task(&task_id, "附件音轨转写失败".into());
                return Err("附件音轨转写失败".into());
            }
        }
    }
    Err("当前模型不支持原始音频，且没有可用的音频转写账号".into())
}

fn transcript_path(state: &ServerState, key: &str, data: &str, format: &str) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(crate::access_control::hash_secret(key));
    digest.update(format);
    digest.update(data);
    state
        .file_root
        .join(".transcripts")
        .join(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cached_transcripts_are_encrypted_and_isolated_by_caller_and_content() {
        let state = crate::protocol_api_tests::state();
        let path = transcript_path(&state, "key-a", "YXVkaW8=", "wav");
        let cipher = state.credential_cipher.encrypt("已识别的音轨").unwrap();
        crate::account_login_credentials::write_private_file(&path, cipher.as_bytes())
            .await
            .unwrap();
        assert!(!tokio::fs::read_to_string(&path)
            .await
            .unwrap()
            .contains("已识别"));
        assert_eq!(
            transcribe(&state, "key-a", "YXVkaW8=", "wav")
                .await
                .unwrap(),
            "已识别的音轨"
        );
        assert!(transcribe(&state, "key-b", "YXVkaW8=", "wav")
            .await
            .is_err());
        assert!(transcribe(&state, "key-a", "ZGlmZmVyZW50", "wav")
            .await
            .is_err());
    }
}
