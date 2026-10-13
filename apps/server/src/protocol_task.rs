pub(crate) use crate::protocol_queue::reserve_affinity_wait;
use crate::store::AppStore;
use crate::{server_plugin_host::ServerPluginHost, store::AccountRecord, ServerState};
use actix_web::web;
use mediagen_core::{PluginManifest, TaskKind};
use mediagen_plugin_runtime::run_js_step;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

/// 请求被取消或执行器意外退出时释放账号占用。
pub(crate) struct Lease {
    pub store: Arc<AppStore>,
    pub id: String,
}
impl Lease {
    pub(crate) fn new(store: Arc<AppStore>, id: String) -> Self {
        store
            .protocol_leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone());
        Self { store, id }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self
            .store
            .task_by_id(&self.id)
            .is_some_and(|task| task.status == "processing")
        {
            self.store
                .fail_task(&self.id, "请求中断或客户端取消".into());
        }
        self.store
            .protocol_leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

pub(crate) fn reserve(
    state: &crate::ServerState,
    manifest: &mediagen_core::PluginManifest,
    model_id: &str,
    kind: &str,
    key: &str,
    id: &str,
    requirements: Option<&serde_json::Value>,
) -> Result<(crate::store::AccountRecord, Lease), String> {
    reserve_affinity(state, manifest, model_id, kind, key, id, requirements, None)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reserve_affinity(
    state: &crate::ServerState,
    manifest: &mediagen_core::PluginManifest,
    model_id: &str,
    kind: &str,
    key: &str,
    id: &str,
    requirements: Option<&serde_json::Value>,
    preferred: Option<&str>,
) -> Result<(crate::store::AccountRecord, Lease), String> {
    let account = state.store.with_account_dispatch(|| {
        let original = state.store.task_by_id(id).and_then(|task| task.account_id);
        if let Some(original) = &original {
            if state.store.account_by_id(original).is_none() {
                return Err("原任务账号不存在，无法读取恢复凭据".into());
            }
        }
        let mut excluded = state
            .store
            .list_accounts(Some(&manifest.platform_id))
            .into_iter()
            .filter(|account| {
                state
                    .store
                    .account_with_required_runtime_proxy(account.clone())
                    .is_err()
                    || crate::model_api::account_model(state, manifest, model_id, &account.id)
                        .is_none_or(|model| {
                            requirements.is_some_and(|body| {
                                crate::protocol_request::validate_capabilities(
                                    body,
                                    model
                                        .extra
                                        .get("capabilities")
                                        .unwrap_or(&serde_json::Value::Null),
                                )
                                .is_err()
                            })
                        })
            })
            .map(|account| account.id)
            .collect::<Vec<_>>();
        if let Some(original) = &original {
            excluded.extend(
                state
                    .store
                    .list_accounts(Some(&manifest.platform_id))
                    .into_iter()
                    .filter(|account| &account.id != original)
                    .map(|account| account.id),
            );
        }
        if let Some(preferred) = original
            .as_deref()
            .or(preferred)
            .and_then(|id| state.store.account_by_id(id))
            .filter(|a| {
                a.platform == manifest.platform_id
                    && !excluded.contains(&a.id)
                    && crate::store_helpers::account_ready_for_kind(a, &[], kind)
                    && (state.store.global_api_key_allowed(key)
                        || crate::access_control::account_key_allowed(&a.api_key, key))
            })
        {
            excluded.extend(
                state
                    .store
                    .list_accounts(Some(&manifest.platform_id))
                    .into_iter()
                    .filter(|a| a.id != preferred.id)
                    .map(|a| a.id),
            );
        }
        let account = state
            .store
            .select_account(&manifest.platform_id, Some(key), &excluded, kind)
            .ok_or("没有可用账号")?;
        let account = state.store.account_with_required_runtime_proxy(account)?;
        state.store.mark_account_dispatched(&account.id);
        if !state
            .store
            .dispatch_protocol_task(crate::store::TaskRecord {
                id: id.into(),
                platform: manifest.platform_id.clone(),
                task_kind: kind.into(),
                model_id: model_id.into(),
                prompt: String::new(),
                account_id: Some(account.id.clone()),
                status: "processing".into(),
                result_url: None,
                storage_key: None,
                error_message: None,
                result_data: None,
                created_at: chrono::Utc::now().to_rfc3339(),
                completed_at: None,
            })
        {
            return Err("请求已在执行，请等待原请求结果".into());
        }
        Ok::<_, String>(account)
    })?;
    Ok((account, Lease::new(state.store.clone(), id.into())))
}

pub(crate) async fn cancelled(store: &AppStore, id: &str) {
    loop {
        if store
            .task_by_id(id)
            .is_some_and(|task| task.status == "cancelled")
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_plugin(
    state: &ServerState,
    manifest: &PluginManifest,
    account: &AccountRecord,
    flow_name: &str,
    request: Value,
    events: Option<mpsc::Sender<Value>>,
    request_id: &str,
    capabilities: Value,
) -> Result<Value, String> {
    let flow = manifest.flows.get(flow_name).ok_or("插件没有配置该能力")?;
    let scripts = state
        .plugin_loader
        .get_plugin_dir(&manifest.platform_id)
        .ok_or("插件目录不存在")?
        .join("scripts");
    let mut context = json!({"account":account,"account_id":account.id,"profile_dir":account.profile_dir,"request":request,"request_id":request_id,"task_id":request_id,"scripts_dir":scripts.to_string_lossy(),"platform_config":state.store.settings().platform_generation_config(&manifest.platform_id),"model_capabilities":capabilities});
    let temporary = request_id
        .is_empty()
        .then(tempfile::tempdir)
        .transpose()
        .map_err(|_| "创建工作目录失败")?;
    let workspace = if let Some(temporary) = &temporary {
        temporary.path().to_owned()
    } else {
        crate::server_plugin_host::recovery_workspace(&account.profile_dir, request_id)?
    };
    context["workspace_dir"] = json!(workspace);
    let replay_events = events.clone();
    let host = ServerPluginHost::new_for_account(account)
        .with_storage(
            state.credential_cipher.clone(),
            &account.profile_dir,
            manifest,
        )
        .with_http_profile(manifest)
        .with_events(events, request_id.into());
    let saved = host.checkpoint(&json!({})).await?;
    if saved["phase"] == "completed" && saved["result"].is_object() {
        let mut result = saved["result"].clone();
        if result["response"].is_object() {
            replay_response(replay_events, &result["response"]).await?;
            result["terminal_event"] = json!({"type":if result["response"]["status"] == "incomplete" {"response.incomplete"} else {"response.completed"},"response":result["response"]});
        }
        return Ok(result);
    }
    let execution = async {
        let mut results = Vec::new();
        for step in &flow.steps {
            let Some(prepared) =
                crate::flow_runtime::prepare_flow_step(step, &mut context, &mut results)?
            else {
                continue;
            };
            if prepared.action != "plugin.run_js" {
                return Err("当前协议需要 JS 插件流程".into());
            }
            let script = prepared.params["script"]
                .as_str()
                .ok_or("缺少插件脚本")?
                .to_string();
            let timeout = prepared.params["timeout_ms"]
                .as_u64()
                .unwrap_or(600_000)
                .min(14_400_000);
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(timeout),
                run_js_step(script, context.clone(), host.clone()),
            )
            .await
            .map_err(|_| "插件执行超时".to_string())??;
            if crate::flow_runtime::flow_result_is_error(&result) {
                return Err(crate::flow_runtime::flow_result_message(&result));
            }
            crate::flow_runtime::record_flow_result(
                &mut context,
                &mut results,
                &prepared.params,
                result,
            );
        }
        let mut result = results.pop().ok_or("插件没有返回结果")?;
        if let Some(event) = host.take_terminal_event() {
            result["terminal_event"] = event;
        }
        host.checkpoint(&json!({"value":{"phase":"completed","result":result}}))
            .await?;
        Ok::<_, String>(result)
    }
    .await;
    match execution {
        Err(message) if host.safe_to_retry().await => Err(format!("SAFE_RETRY:{message}")),
        result => result,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_with_failover(
    state: &web::Data<ServerState>,
    manifest: &PluginManifest,
    account: &mut AccountRecord,
    flow: &str,
    request: Value,
    events: Option<mpsc::Sender<Value>>,
    task_id: &str,
    capabilities: Value,
    key: &str,
) -> Result<Value, String> {
    let mut excluded = Vec::new();
    for attempt in 0..3 {
        let actual = crate::model_api::account_model(
            state,
            manifest,
            request["model"].as_str().unwrap_or(""),
            &account.id,
        );
        let actual_caps = actual
            .as_ref()
            .and_then(|model| model.extra.get("capabilities"))
            .cloned()
            .unwrap_or_else(|| capabilities.clone());
        let result = run_plugin(
            state,
            manifest,
            account,
            flow,
            request.clone(),
            events.clone(),
            task_id,
            actual_caps,
        )
        .await;
        let Err(message) = &result else {
            return result;
        };
        let Some(model) = actual else {
            return result;
        };
        let context = crate::media_execution::MediaFlowContext {
            manifest,
            model: &model,
            account: account.clone(),
            task_id: task_id.into(),
        };
        let decision = crate::media_execution::classify_and_apply_failure(state, &context, message);
        if decision.usage_consumed {
            state.store.mark_account_usage_for(
                &account.id,
                if model.task_kind == TaskKind::Text {
                    "text"
                } else {
                    "media"
                },
            );
        }
        if attempt == 2 || !message.starts_with("SAFE_RETRY:") || decision.usage_consumed {
            return result;
        }
        if decision.should_retry_same_account(attempt + 1) {
            tokio::time::sleep(std::time::Duration::from_millis(250 * (attempt + 1) as u64)).await;
            continue;
        }
        if request["options"]["platform_session"].is_object() || !decision.should_failover() {
            return result;
        }
        excluded.push(account.id.clone());
        let next = state.store.with_account_dispatch(|| {
            let mut rejected = excluded.clone();
            for candidate in state.store.list_accounts(Some(&manifest.platform_id)) {
                if state
                    .store
                    .account_with_required_runtime_proxy(candidate.clone())
                    .is_err()
                    || crate::model_api::account_model(
                        state,
                        manifest,
                        &model.model_id,
                        &candidate.id,
                    )
                    .is_none_or(|candidate_model| {
                        crate::protocol_request::validate_capabilities(
                            &request,
                            candidate_model
                                .extra
                                .get("capabilities")
                                .unwrap_or(&Value::Null),
                        )
                        .is_err()
                    })
                {
                    rejected.push(candidate.id);
                }
            }
            let kind = if model.task_kind == TaskKind::Text {
                "text"
            } else {
                "audio"
            };
            let next =
                state
                    .store
                    .select_account(&manifest.platform_id, Some(key), &rejected, kind)?;
            let next = state.store.account_with_required_runtime_proxy(next).ok()?;
            state.store.set_task_account(task_id, &next.id);
            state.store.mark_account_dispatched(&next.id);
            Some(next)
        });
        let Some(next) = next else {
            return result;
        };
        *account = next;
    }
    Err("重试次数已耗尽".into())
}

/// 用已保存的完整响应重放协议事件，不再次请求上游。
async fn replay_response(
    events: Option<mpsc::Sender<Value>>,
    response: &Value,
) -> Result<(), String> {
    let Some(sender) = events else { return Ok(()) };
    let mut created = response.clone();
    created["output"] = json!([]);
    created["status"] = json!("in_progress");
    let mut frames = vec![json!({"type":"response.created","response":created})];
    for (index, item) in response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        frames.push(json!({"type":"response.output_item.added","output_index":index,"item":item}));
        if item["type"] == "function_call" {
            frames.push(json!({"type":"response.function_call_arguments.delta","item_id":item["id"],"delta":item["arguments"]}));
        }
        for part in item["content"].as_array().into_iter().flatten() {
            if part["type"] == "output_text" {
                frames.push(json!({"type":"response.output_text.delta","item_id":item["id"],"output_index":index,"delta":part["text"]}));
            }
        }
        frames.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    for frame in frames {
        sender.send(frame).await.map_err(|_| "请求已取消")?;
    }
    Ok(())
}
