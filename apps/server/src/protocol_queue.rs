//! 同步协议复用任务记录等待账号，不改变 Responses 返回契约。
use crate::{
    protocol_task::{reserve_affinity, Lease},
    store::{AccountRecord, AppStore, TaskRecord},
    ServerState,
};
use mediagen_core::PluginManifest;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// 并发占用可以排队；没有权限、登录态或额度不能无限等待。
pub(crate) fn check_model_accounts(
    state: &ServerState,
    manifest: &PluginManifest,
    model: &str,
    kind: &str,
    key: &str,
    bound: Option<&str>,
) -> Result<(), String> {
    let mut permitted = false;
    for mut account in state.store.list_accounts(Some(&manifest.platform_id)) {
        if bound.is_some_and(|id| id != account.id)
            || !(state.store.global_api_key_allowed(key)
                || crate::access_control::account_key_allowed(&account.api_key, key))
            || matches!(
                account.login_state.as_str(),
                "disabled" | "login_expired" | "verification_required"
            )
            || state
                .store
                .account_with_required_runtime_proxy(account.clone())
                .is_err()
            || crate::model_api::account_model(state, manifest, model, &account.id).is_none()
        {
            continue;
        }
        permitted = true;
        account.task_interval_seconds = 0;
        if crate::store_helpers::account_ready_for_kind(&account, &[], kind) {
            return Ok(());
        }
    }
    if !permitted {
        Err(format!("no_available_account_for_model: 没有账号可以使用模型 {model}，请检查模型权限、登录状态和代理配置"))
    } else {
        Err(format!("insufficient_quota: 没有账号具备模型 {model} 的可用额度，账号已达限额或处于冷却中，请恢复额度后重新请求"))
    }
}

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
    let bound = crate::protocol_task::bound_account(state, id, requirements, preferred)?;
    if kind == "text" {
        check_model_accounts(state, manifest, model, kind, key, bound.as_deref())?;
    }
    // 持久队列负责等待和重试，执行尝试只复用账号准入，不再嵌套同步等待队列。
    if state.store.generation_job(id, None).is_some() {
        return reserve_affinity(
            state,
            manifest,
            model,
            kind,
            key,
            id,
            requirements,
            preferred,
        );
    }
    // 新同步请求先尝试空闲账号；其他账号上的等待请求不能阻塞它。
    // 同一任务已在等待时仍由原请求处理，防止重复请求抢占其执行器。
    let pending = state
        .store
        .task_by_id(id)
        .is_some_and(|task| task.status == "queued");
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
    let eligible = || {
        state
            .store
            .list_accounts(Some(&manifest.platform_id))
            .into_iter()
            .any(|account| {
                bound.as_ref().is_none_or(|id| id == &account.id)
                    && crate::store_quota::account_can_wait_for_kind(&account, kind)
                    && (state.store.global_api_key_allowed(key)
                        || crate::access_control::account_key_allowed(&account.api_key, key))
                    && state
                        .store
                        .account_with_required_runtime_proxy(account.clone())
                        .is_ok()
                    && crate::model_api::account_model(state, manifest, model, &account.id)
                        .is_some_and(|model| {
                            requirements.is_none_or(|request| {
                                crate::protocol_request::validate_capabilities(
                                    request,
                                    model.extra.get("capabilities").unwrap_or(&Value::Null),
                                )
                                .is_ok()
                            })
                        })
            })
    };
    if !eligible() {
        return Err("没有已就绪且有可用额度的账号，请检查账号、代理和模型配置".into());
    }
    state.store.with_account_dispatch(|| {
        if state.store.task_by_id(id).is_some_and(|task| {
            matches!(task.status.as_str(), "processing" | "queued" | "cancelled")
        }) || state
            .store
            .protocol_leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(id)
        {
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
        state.store.dispatch_protocol_task(TaskRecord {
            id: id.into(),
            platform: manifest.platform_id.clone(),
            task_kind: kind.into(),
            model_id: model.into(),
            prompt: String::new(),
            account_id: bound.clone(),
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
    loop {
        if state
            .store
            .task_by_id(id)
            .is_none_or(|task| task.status != "queued")
        {
            return Err("请求已停止等待，请检查原任务状态".into());
        }
        if kind == "text" {
            check_model_accounts(state, manifest, model, kind, key, bound.as_deref())?;
        }
        if !eligible() {
            return Err("账号已停用、失效或无可恢复额度，请检查账号配置".into());
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
        tokio::select! {
            _ = state.store.dispatch_notify.notified() => {},
            _ = tokio::time::sleep(Duration::from_millis(250)) => {},
        }
    }
}

#[cfg(test)]
#[path = "protocol_queue_tests.rs"]
mod tests;
