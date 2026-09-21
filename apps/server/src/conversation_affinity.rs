use crate::{store::AppStore, ServerState};
use actix_web::{HttpMessage, HttpRequest};
use serde_json::{json, Value};

pub(crate) struct ConnectionId(pub String);

/// 绑定按调用者和平台隔离；不存原始对话或 API Key。
pub(crate) fn identify(
    state: &ServerState,
    request: &HttpRequest,
    key: &str,
    platform: &str,
    body: &Value,
    payload: &Value,
) -> (String, Option<String>) {
    let explicit = body["prompt_cache_key"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            request
                .extensions()
                .get::<ConnectionId>()
                .map(|v| v.0.clone())
        });
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
    async fn affinity_is_owned_stays_sticky_and_escapes_expired_accounts() {
        let state = crate::protocol_api_tests::state();
        state
            .store
            .insert_account(crate::test_fixtures::account_record(
                "second", "test", "Second", "",
            ));
        let request = actix_web::test::TestRequest::default().to_http_request();
        let payload = json!({"input":"hello","prompt_cache_key":"session"});
        let (key, preferred) = identify(&state, &request, "key-a", "test", &payload, &payload);
        assert!(preferred.is_none());
        remember(&state.store, &key, "account");
        let (_, preferred) = identify(&state, &request, "key-a", "test", &payload, &payload);
        assert_eq!(preferred.as_deref(), Some("account"));
        let (_, other) = identify(&state, &request, "key-b", "test", &payload, &payload);
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
