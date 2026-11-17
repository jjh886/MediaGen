use actix_web::web;
use mediagen_core::PluginManifest;
use serde_json::{json, Value};

use crate::{server_plugin_host::ServerPluginHost, ServerState};

fn history_path(state: &ServerState, session: &str, task: &str) -> std::path::PathBuf {
    state.file_root.join("conversation-sessions").join(format!(
        "{session}-{}.history",
        crate::access_control::hash_secret(task)
    ))
}

/// 客户端可以只发送本轮消息；重建时优先采用服务端确认过的完整历史。
pub(crate) fn restore(
    state: &ServerState,
    session: &str,
    saved: &Value,
    request: &mut Value,
    previous_response: bool,
) -> Result<bool, String> {
    if previous_response {
        return Ok(true);
    }
    let input = crate::protocol_storage::input_items(&request["input"]);
    let start =
        crate::conversation_affinity::input_start(&json!(input), saved["response_anchor"].as_str());
    let task = saved["task_id"].as_str().unwrap_or_default();
    let history = match std::fs::read_to_string(history_path(state, session, task)) {
        Ok(encrypted) => {
            serde_json::from_str::<Vec<Value>>(&state.credential_cipher.decrypt(&encrypted)?)
                .map_err(|_| "会话历史损坏，不能安全迁移".to_string())?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let prior = state
                .store
                .task_by_id(task)
                .and_then(|t| t.result_data)
                .and_then(|v| serde_json::from_str::<Value>(&v).ok());
            if let Some(prior) =
                prior.filter(|v| v["input"].is_array() && v["response"].is_object())
            {
                let mut history = crate::protocol_storage::input_items(&prior["input"]);
                history.extend(crate::protocol_output::continuation(&prior["response"]));
                history
            } else {
                // 旧版未保存 Chat 历史时，仅接受客户端提供的完整前文，不能只迁移最后一句。
                return Ok(start > 0
                    || (input.iter().any(|item| item["role"] == "assistant")
                        && input.iter().filter(|item| item["role"] == "user").count() >= 2));
            }
        }
        Err(_) => return Err("读取会话历史失败".into()),
    };
    if input.starts_with(&history) {
        return Ok(true);
    }
    let mut complete = history;
    complete.extend(input.into_iter().skip(start));
    request["input"] = json!(complete);
    Ok(true)
}

pub(crate) fn fresh_request(request: &Value) -> Value {
    let mut fresh = request.clone();
    if let Some(options) = fresh["options"].as_object_mut() {
        options.remove("platform_session");
        options.remove("session_input_start");
    }
    // 原账号的加密推理状态不可转交其他账号；用户消息、工具结果和附件保持完整。
    if let Some(input) = fresh["input"].as_array_mut() {
        input.retain(|item| item["type"] != "reasoning" || item["encrypted_content"].is_null());
    }
    fresh
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn route(
    state: &ServerState,
    manifest: &PluginManifest,
    key: &str,
    task: &str,
    request: &mut Value,
    preferred: &mut Option<String>,
    complete: bool,
) -> Result<bool, String> {
    if !request["options"]["platform_session"].is_object() {
        return Ok(false);
    }
    let original = state.store.task_by_id(task).and_then(|t| t.account_id);
    if original
        .as_ref()
        .is_some_and(|account| Some(account) != preferred.as_ref())
    {
        // 换号后的中断请求必须恢复新账号检查点，不能重新套用旧账号游标。
        *request = fresh_request(request);
        *preferred = original;
        return Ok(true);
    }
    let limited = crate::protocol_queue::check_model_accounts(
        state,
        manifest,
        request["model"].as_str().unwrap_or_default(),
        "text",
        key,
        preferred.as_deref(),
    )
    .is_err_and(|error| error.starts_with("insufficient_quota:"));
    if !limited {
        return Ok(false);
    }
    if let Some(account) = original.and_then(|id| state.store.account_by_id(&id)) {
        let host = ServerPluginHost::new_for_account(&account)
            .with_storage(
                state.credential_cipher.clone(),
                &account.profile_dir,
                manifest,
            )
            .with_events(None, task.into());
        if !host.safe_to_retry().await {
            return Ok(false);
        }
    }
    if !complete {
        return Err(
            "conversation_context_unavailable: 旧会话没有可恢复的完整历史，请携带完整历史重新请求"
                .into(),
        );
    }
    *request = fresh_request(request);
    request["options"]["migrate_account"] = json!(true);
    *preferred = None;
    Ok(true)
}

pub(crate) async fn prepare(
    state: &web::Data<ServerState>,
    key: &str,
    task: &str,
    raw: &Value,
    capabilities: &Value,
) -> Result<(Value, crate::upload_cleanup::UploadUse), String> {
    let mut request = fresh_request(raw);
    let guard = crate::upload_cleanup::register(state, task, key, &request).await
        .map_err(|_| "conversation_attachment_unavailable: 旧会话附件已清理或无权读取，无法完整迁移，请重新上传附件".to_string())?;
    crate::protocol_files::resolve(state, key, &mut request, capabilities)
        .await
        .map_err(|_| {
            "conversation_attachment_unavailable: 无法读取旧会话附件，请重新上传附件".to_string()
        })?;
    crate::protocol_attachments::prepare(state, key, &mut request, capabilities).await?;
    crate::protocol_request::validate_capabilities(&request, capabilities)?;
    Ok((request, guard))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn save(
    state: &ServerState,
    session: &str,
    account: &str,
    task: &str,
    base: Option<&str>,
    input: &Value,
    response: &Value,
) -> Result<(), String> {
    if !response["metadata"]["platform_session"].is_object() {
        return Ok(());
    }
    let previous = crate::conversation_affinity::read_session(state, session)?;
    if previous
        .as_ref()
        .is_some_and(|v| v["task_id"] != task && v["task_id"].as_str() != base)
    {
        return Ok(());
    }
    let mut history = crate::protocol_storage::input_items(input);
    history.extend(crate::protocol_output::continuation(response));
    let encrypted = state
        .credential_cipher
        .encrypt(&serde_json::to_string(&history).map_err(|_| "会话历史编码失败")?)?;
    crate::account_login_credentials::write_private_file(
        &history_path(state, session, task),
        encrypted.as_bytes(),
    )
    .await?;
    crate::conversation_affinity::write_session(state, session, account, task, base, response)
        .await?;
    if let Some(old) = previous
        .as_ref()
        .and_then(|v| v["task_id"].as_str())
        .filter(|old| *old != task)
    {
        match tokio::fs::remove_file(history_path(state, session, old)).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(%e, "旧会话历史副本清理失败"),
        }
    }
    Ok(())
}

pub(crate) async fn remove(state: &ServerState, session: &str) -> Result<(), String> {
    if let Some(saved) = crate::conversation_affinity::read_session(state, session)? {
        if let Some(task) = saved["task_id"].as_str() {
            let path = history_path(state, session, task);
            if let Err(e) = tokio::fs::remove_file(path).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err("删除会话历史失败".into());
                }
            }
        }
        tokio::fs::remove_file(
            state
                .file_root
                .join("conversation-sessions")
                .join(format!("{session}.json")),
        )
        .await
        .map_err(|_| "删除会话绑定失败")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "conversation_replay_tests.rs"]
mod tests;
