use crate::{store::AppStore, ServerState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) struct ConnectionId(pub String);

/// 绑定按调用者和平台隔离；不存原始对话或 API Key。
pub(crate) fn identify(
    state: &ServerState,
    connection_id: Option<&str>,
    key: &str,
    platform: &str,
    body: &Value,
    payload: &Value,
) -> (String, Option<String>) {
    let window = (body["metadata"]["duanju_mode"] == "chat")
        .then(|| body["metadata"]["duanju_thread_id"].as_str())
        .flatten()
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    let explicit = window
        .or_else(|| {
            body["prompt_cache_key"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| connection_id.map(str::to_owned));
    let input = crate::protocol_storage::input_items(&payload["input"]);
    let first = input
        .iter()
        .find(|item| item["role"] == "user")
        .cloned()
        .unwrap_or(Value::Null);
    let identity = explicit
        .map(|v| json!({"session":v}))
        .unwrap_or_else(|| json!({"first_user":first}));
    let id = crate::access_control::hash_secret(&json!([key, platform, identity]).to_string());
    let previous = body["previous_response_id"]
        .as_str()
        .and_then(|previous| crate::protocol_storage::stored(state, key, previous))
        .and_then(|value| {
            value["task_id"]
                .as_str()
                .and_then(|task| state.store.task_by_id(task))
        })
        .and_then(|task| task.account_id);
    let cached = state
        .store
        .session_affinity
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
        .filter(|(_, at)| chrono::Utc::now().timestamp() - at < 86400)
        .map(|(account, _)| account.clone());
    (id, previous.or(cached))
}

/// 明确的窗口标识或 Responses 续接才共享平台会话；缓存键和相同正文不能合并窗口。
pub(crate) fn session_identity(
    state: &ServerState,
    connection_id: Option<&str>,
    key: &str,
    platform: &str,
    body: &Value,
    endpoint: &str,
    task_id: &str,
) -> Option<String> {
    if !matches!(endpoint, "responses" | "chat") {
        return None;
    }
    let window = (body["metadata"]["duanju_mode"] == "chat")
        .then(|| body["metadata"]["duanju_thread_id"].as_str())
        .flatten()
        .or_else(|| body["metadata"]["session_id"].as_str())
        .filter(|id| !id.trim().is_empty());
    if let Some(window) = window {
        return Some(crate::access_control::hash_secret(
            &json!([key, platform, {"session":window}]).to_string(),
        ));
    }
    if let Some(prior) = body["previous_response_id"]
        .as_str()
        .and_then(|id| crate::protocol_storage::stored(state, key, id))
        .and_then(|value| value["conversation_session_id"].as_str().map(str::to_owned))
    {
        return Some(prior);
    }
    let identity = if let Some(connection) = connection_id {
        json!({"connection":connection})
    } else if endpoint == "responses" && body["store"] != false {
        json!({"request":task_id})
    } else {
        return None;
    };
    Some(crate::access_control::hash_secret(
        &json!([key, platform, identity]).to_string(),
    ))
}

pub(crate) fn remember(store: &AppStore, id: &str, account: &str) {
    let now = chrono::Utc::now().timestamp();
    let mut entries = store
        .session_affinity
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    entries.retain(|_, (_, at)| now - *at < 86400);
    if entries.len() >= 10000 {
        if let Some(oldest) = entries
            .iter()
            .min_by_key(|(_, (_, at))| at)
            .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
    }
    entries.insert(id.into(), (account.into(), now));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[actix_web::test]
    async fn explicit_session_is_scoped_and_never_inferred_from_cache_or_prompt() {
        let state = crate::protocol_api_tests::state();
        let body = json!({"metadata":{"session_id":"window"}});
        let first = session_identity(&state, None, "owner-a", "test", &body, "chat", "request");
        assert!(first.is_some());
        assert_eq!(
            first,
            session_identity(
                &state,
                None,
                "owner-a",
                "test",
                &body,
                "responses",
                "request"
            )
        );
        assert_ne!(
            first,
            session_identity(&state, None, "owner-b", "test", &body, "chat", "request")
        );
        assert_ne!(
            first,
            session_identity(&state, None, "owner-a", "other", &body, "chat", "request")
        );
        assert!(
            session_identity(&state, None, "owner-a", "test", &body, "compact", "request")
                .is_none()
        );
        let unbound = json!({"input":"same","prompt_cache_key":"shared","store":false});
        assert!(session_identity(
            &state,
            None,
            "owner-a",
            "test",
            &unbound,
            "responses",
            "request"
        )
        .is_none());
    }
    #[actix_web::test]
    async fn affinity_is_owned_stays_sticky_and_escapes_expired_accounts() {
        let state = crate::protocol_api_tests::state();
        state
            .store
            .insert_account(crate::test_fixtures::account_record(
                "second", "test", "Second", "",
            ));
        let payload = json!({"input":"hello","prompt_cache_key":"session"});
        let (key, preferred) = identify(&state, None, "key-a", "test", &payload, &payload);
        assert!(preferred.is_none());
        remember(&state.store, &key, "account");
        let (_, preferred) = identify(&state, None, "key-a", "test", &payload, &payload);
        assert_eq!(preferred.as_deref(), Some("account"));
        let (_, other) = identify(&state, None, "key-b", "test", &payload, &payload);
        assert!(other.is_none());
        let manifest = state.plugin_loader.get_plugin("test").unwrap();
        for id in ["turn1", "turn2"] {
            let (account, lease) = crate::protocol_task::reserve_affinity(
                &state,
                manifest,
                "text-model",
                "text",
                "key-a",
                id,
                None,
                preferred.as_deref(),
            )
            .unwrap();
            assert_eq!(account.id, "account");
            state.store.complete_task(id, json!({"status":"completed"}));
            drop(lease);
        }
        state.store.mark_account_state("account", "login_expired");
        let (account, _lease) = crate::protocol_task::reserve_affinity(
            &state,
            manifest,
            "text-model",
            "text",
            "key-a",
            "turn3",
            None,
            preferred.as_deref(),
        )
        .unwrap();
        assert_eq!(account.id, "second");
    }
}

/// 仅显式窗口标识启用持久平台会话；不能仅凭相同提示词合并两个窗口。
pub(crate) fn read_session(state: &ServerState, id: &str) -> Result<Option<Value>, String> {
    let path = state
        .file_root
        .join("conversation-sessions")
        .join(format!("{id}.json"));
    let encrypted = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("读取平台会话绑定失败".into()),
    };
    let value: Value = serde_json::from_str(&state.credential_cipher.decrypt(&encrypted)?)
        .map_err(|_| "平台会话绑定损坏")?;
    if !value["account_id"].is_string() || !value["cursor"].is_object() {
        return Err("平台会话绑定无效".into());
    }
    Ok(Some(value))
}
pub(crate) async fn write_session(
    state: &ServerState,
    id: &str,
    account: &str,
    task: &str,
    base: Option<&str>,
    response: &Value,
) -> Result<(), String> {
    if let Some(current) = read_session(state, id)? {
        if current["task_id"] != task && current["task_id"].as_str() != base {
            return Ok(());
        }
    }
    let cursor = &response["metadata"]["platform_session"];
    if !cursor.is_object() {
        return Ok(());
    }
    let payload = json!({"account_id":account,"task_id":task,"cursor":cursor,"response_anchor":response_anchor(response)});
    if payload.to_string().len() > 65536 {
        return Err("平台会话信息过大".into());
    }
    let encrypted = state.credential_cipher.encrypt(&payload.to_string())?;
    crate::account_login_credentials::write_private_file(
        &state
            .file_root
            .join("conversation-sessions")
            .join(format!("{id}.json")),
        encrypted.as_bytes(),
    )
    .await
}

/// 为一次请求冻结前驱关系，旧请求重放不得覆盖较新的对话状态。
pub(crate) async fn begin_session(
    state: &ServerState,
    id: &str,
    task: &str,
) -> Result<Option<String>, String> {
    let path = state
        .file_root
        .join("conversation-sessions")
        .join(format!("{id}-{task}.base"));
    match tokio::fs::read_to_string(&path).await {
        Ok(encrypted) => serde_json::from_str(&state.credential_cipher.decrypt(&encrypted)?)
            .map_err(|_| "会话请求关联损坏".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let base = read_session(state, id)?
                .and_then(|value| value["task_id"].as_str().map(str::to_owned));
            let encrypted = state
                .credential_cipher
                .encrypt(&serde_json::to_string(&base).map_err(|_| "会话请求关联编码失败")?)?;
            crate::account_login_credentials::write_private_file(&path, encrypted.as_bytes())
                .await?;
            Ok(base)
        }
        Err(_) => Err("读取会话请求关联失败".into()),
    }
}

fn content_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("")
}
fn text_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.trim().as_bytes()))
}
fn response_anchor(response: &Value) -> Option<String> {
    let text = response["output_text"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            response["output"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|item| item["type"] == "message")
                .map(|item| content_text(&item["content"]))
                .collect::<Vec<_>>()
                .join("")
        });
    (!text.trim().is_empty()).then(|| text_hash(&text))
}
// 其他平台或模式可能在两次调用之间补入消息；仅切掉已确认存入当前平台的部分。
pub(crate) fn input_start(input: &Value, anchor: Option<&str>) -> usize {
    let Some(anchor) = anchor else { return 0 };
    let matches = input
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, item)| {
            item["role"] == "assistant" && text_hash(&content_text(&item["content"])) == anchor
        })
        .map(|(index, _)| index + 1)
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches[0]
    } else {
        0
    }
}

pub(crate) fn apply_cursor(payload: &mut Value, saved: &Value) -> Option<String> {
    if !payload["options"].is_object() {
        payload["options"] = json!({});
    }
    payload["options"]["platform_session"] = saved["cursor"].clone();
    payload["options"]["session_input_start"] = json!(input_start(
        &payload["input"],
        saved["response_anchor"].as_str()
    ));
    saved["account_id"].as_str().map(str::to_owned)
}

// 会话游标属于宿主内部状态，不作为 OpenAI 字符串 metadata 扩展暴露给客户端。
pub(crate) fn redact_cursor(value: &mut Value) {
    if let Some(metadata) = value
        .get_mut("response")
        .and_then(|response| response.get_mut("metadata"))
        .and_then(Value::as_object_mut)
    {
        metadata.remove("platform_session");
    }
}
