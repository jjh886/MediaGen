//! 同步协议复用任务记录等待账号，不改变 Responses 返回契约。
use crate::{
    protocol_task::{reserve_affinity, Lease},
    store::{AccountRecord, AppStore, TaskRecord},
    ServerState,
};
use mediagen_core::PluginManifest;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

struct Waiting {
    store: Arc<AppStore>,
    id: String,
}
impl Drop for Waiting {
    fn drop(&mut self) {
        if self
            .store
            .task_by_id(&self.id)
            .is_some_and(|task| task.status == "queued")
        {
            self.store
                .fail_task(&self.id, "等待已结束，可继续原请求".into());
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn reserve_affinity_wait(
    state: &ServerState,
    manifest: &PluginManifest,
    model: &str,
    kind: &str,
    key: &str,
    id: &str,
    requirements: Option<&Value>,
    preferred: Option<&str>,
) -> Result<(AccountRecord, Lease), String> {
    let pending = state
        .store
        .list_tasks(Some(&manifest.platform_id))
        .iter()
        .any(|task| task.status == "queued" && task.task_kind == kind && task.model_id == model);
    if !pending {
        if let Ok(value) = reserve_affinity(
            state,
            manifest,
            model,
            kind,
            key,
            id,
            requirements,
            preferred,
        ) {
            return Ok(value);
        }
    }
    let eligible = state
        .store
        .list_accounts(Some(&manifest.platform_id))
        .into_iter()
        .any(|account| {
            crate::store_helpers::account_ready_for_kind(&account, &[], kind)
                && (state.store.global_api_key_allowed(key)
                    || crate::access_control::account_key_allowed(&account.api_key, key))
                && state
                    .store
                    .account_with_required_runtime_proxy(account.clone())
                    .is_ok()
                && crate::model_api::account_model(state, manifest, model, &account.id).is_some_and(
                    |model| {
                        requirements.is_none_or(|request| {
                            crate::protocol_request::validate_capabilities(
                                request,
                                model.extra.get("capabilities").unwrap_or(&Value::Null),
                            )
                            .is_ok()
                        })
                    },
                )
        });
    if !eligible {
        return Err("没有已就绪且有可用额度的账号，请检查账号、代理和模型配置".into());
    }
    state.store.with_account_dispatch(|| {
        if state.store.task_by_id(id).is_some() {
            return Err("请求已在执行，请等待原请求结果".to_owned());
        }
        let queued = state
            .store
            .list_tasks(Some(&manifest.platform_id))
            .iter()
            .filter(|task| task.task_kind == kind && task.status == "queued")
            .count();
        if queued >= 128 {
            return Err("等待处理的请求较多，请稍后继续".into());
        }
        state.store.insert_task(TaskRecord {
            id: id.into(),
            platform: manifest.platform_id.clone(),
            task_kind: kind.into(),
            model_id: model.into(),
            prompt: String::new(),
            account_id: None,
            status: "queued".into(),
            result_url: None,
            storage_key: None,
            error_message: None,
            result_data: Some(json!({"owner":crate::access_control::hash_secret(key)}).to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
        });
        Ok(())
    })?;
    let _waiting = Waiting {
        store: state.store.clone(),
        id: id.into(),
    };
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if state
            .store
            .task_by_id(id)
            .is_none_or(|task| task.status != "queued")
        {
            return Err("请求已停止等待，请检查原任务状态".into());
        }
        let first = state
            .store
            .list_tasks(Some(&manifest.platform_id))
            .into_iter()
            .filter(|task| {
                task.status == "queued" && task.task_kind == kind && task.model_id == model
            })
            .min_by(|a, b| {
                a.created_at
                    .cmp(&b.created_at)
                    .then_with(|| a.id.cmp(&b.id))
            });
        if first.is_some_and(|task| task.id == id) {
            if let Ok(value) = reserve_affinity(
                state,
                manifest,
                model,
                kind,
                key,
                id,
                requirements,
                preferred,
            ) {
                return Ok(value);
            }
        }
        if Instant::now() >= deadline {
            return Err("正在等待空闲账号，请稍后继续，将保留本机处理进度".into());
        }
        tokio::select! {
            _ = state.generation_queue_notify.notified() => {},
            _ = tokio::time::sleep(Duration::from_millis(250)) => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[actix_web::test]
    async fn cancelled_wait_releases_queue_and_unconfigured_accounts_do_not_wait() {
        let state = crate::protocol_api_tests::state();
        state.store.update_account(
            "account",
            crate::store::AccountUpdate {
                text_concurrency_limit: Some(1),
                ..Default::default()
            },
        );
        let (manifest, _, _) = crate::model_api::resolve(&state, "text-model", "key-a").unwrap();
        let (_, busy) = reserve_affinity(
            &state,
            &manifest,
            "text-model",
            "text",
            "key-a",
            "busy-cancel",
            None,
            None,
        )
        .unwrap();
        let waiting = reserve_affinity_wait(
            &state,
            &manifest,
            "text-model",
            "text",
            "key-a",
            "cancel-wait",
            None,
            None,
        );
        assert!(tokio::time::timeout(Duration::from_millis(25), waiting)
            .await
            .is_err());
        assert_eq!(
            state.store.task_by_id("cancel-wait").unwrap().status,
            "failed"
        );
        assert_eq!(
            state
                .store
                .list_tasks(None)
                .iter()
                .filter(|task| task.status == "queued")
                .count(),
            0
        );
        drop(busy);
        state.store.toggle_account("account", true);
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            reserve_affinity_wait(
                &state,
                &manifest,
                "text-model",
                "text",
                "key-a",
                "unconfigured",
                None,
                None,
            ),
        )
        .await
        .unwrap();
        assert!(result.is_err());
        assert!(state.store.task_by_id("unconfigured").is_none());
    }
    #[actix_web::test]
    async fn busy_account_waits_and_dispatches_without_duplicate_records() {
        let state = crate::protocol_api_tests::state();
        let account = state.store.account_by_id("account").unwrap();
        state.store.update_account(
            &account.id,
            crate::store::AccountUpdate {
                text_concurrency_limit: Some(1),
                ..Default::default()
            },
        );
        let (manifest, _, _) = crate::model_api::resolve(&state, "text-model", "key-a").unwrap();
        let (_, lease) = reserve_affinity(
            &state,
            &manifest,
            "text-model",
            "text",
            "key-a",
            "busy",
            None,
            None,
        )
        .unwrap();
        let release = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            state
                .store
                .complete_task("busy", json!({"status":"completed"}));
            drop(lease);
        };
        let wait = reserve_affinity_wait(
            &state,
            &manifest,
            "text-model",
            "text",
            "key-a",
            "next",
            None,
            None,
        );
        let (result, _) = tokio::join!(wait, release);
        let (_, permit) = result.unwrap();
        assert_eq!(
            state
                .store
                .list_tasks(None)
                .iter()
                .filter(|task| task.id == "next")
                .count(),
            1
        );
        assert_eq!(state.store.task_by_id("next").unwrap().status, "processing");
        drop(permit);
    }
}
