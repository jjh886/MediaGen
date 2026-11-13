use crate::{server_plugin_host::ServerPluginHost, store::AccountRecord, ServerState};
use mediagen_core::PluginManifest;
use mediagen_plugin_runtime::run_js_step;
use serde_json::{json, Value};
use tokio::sync::mpsc;

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
    if flow_name == "generate_text"
        && context["model_capabilities"]["attachment_transport"] == "workspace"
    {
        crate::protocol_files::materialize(state, &mut context["request"], &workspace).await?;
    }
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
    if saved["_transport_failures"].as_u64().unwrap_or(0) >= 3 {
        return Err(
            "recovery_exhausted: 原请求连续三次连接失败，已停止自动恢复，请检查网络并核对原任务"
                .into(),
        );
    }
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
    if let Err(message) = &execution {
        let lower = message.to_ascii_lowercase();
        if ["operation timed out", "broken pipe", "connection reset"]
            .iter()
            .any(|kind| lower.contains(kind))
        {
            let mut checkpoint = host.checkpoint(&json!({})).await?;
            checkpoint["_transport_failures"] =
                json!(checkpoint["_transport_failures"].as_u64().unwrap_or(0) + 1);
            host.checkpoint(&json!({"value":checkpoint})).await?;
            if checkpoint["_transport_failures"].as_u64().unwrap_or(0) >= 3 {
                return Err(format!("recovery_exhausted: 连续三次连接失败；{message}"));
            }
        }
    }
    match execution {
        Err(message) if host.safe_to_retry().await => Err(format!("SAFE_RETRY:{message}")),
        result => result,
    }
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
