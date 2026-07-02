use std::path::Path;

use mediagen_core::{FlowDef, PluginManifest};
use mediagen_plugin_runtime::PluginLoader;
use serde_json::{json, Value};

use crate::flow_templates::{collect_script_files, resolve_templates, should_skip};
use crate::image_request::ImageFlowRequest;
use crate::store::AccountRecord;

pub(crate) struct PreparedFlowStep {
    pub action: String,
    pub params: Value,
}

pub(crate) fn generate_image_flow(manifest: &PluginManifest) -> Result<&FlowDef, String> {
    manifest
        .flows
        .get("generate_image")
        .ok_or_else(|| format!("{} 未配置 generate_image flow", manifest.platform_id))
}

pub(crate) fn build_image_flow_context(
    plugin_loader: &PluginLoader,
    manifest: &PluginManifest,
    account: &AccountRecord,
    input: ImageFlowRequest,
) -> Result<Value, String> {
    let scripts_dir = plugin_loader
        .get_plugin_dir(&manifest.platform_id)
        .map(|dir| dir.join("scripts"))
        .ok_or_else(|| format!("没有找到 {} 插件目录", manifest.platform_id))?;
    Ok(json!({
        "account": account,
        "account_id": account.id,
        "profile_dir": account.profile_dir,
        "browser_session_id": format!("task-{}", uuid::Uuid::new_v4()),
        "prompt": input.prompt,
        "image_aspect_ratio": input.image_aspect_ratio,
        "max_images": input.max_images,
        "model_id": input.model_id,
        "model_display_name": input.model_display_name,
        "scripts_dir": scripts_dir.to_string_lossy(),
        "account_cookies": "",
        "ref_image_local": input.ref_image_locals.first().cloned().unwrap_or_default(),
        "ref_image_locals": input.ref_image_locals,
        "ref_image_urls": input.ref_image_urls,
        "ref_image_dataurls": input.ref_image_dataurls,
        "mask_image_url": input.mask_image_url,
        "mask_image_dataurl": input.mask_image_dataurl,
        "workspace_dir": "",
        "output_format": input.output_format,
        "output_compression": input.output_compression,
        "quality": input.quality,
        "background": input.background,
        "partial_images": input.partial_images,
        "moderation": input.moderation,
        "input_fidelity": input.input_fidelity,
        "size": input.size,
        "action": input.action,
    }))
}

pub(crate) fn prepare_flow_step(
    step: &Value,
    context: &mut Value,
    results: &mut Vec<Value>,
) -> Result<Option<PreparedFlowStep>, String> {
    let action = step
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "flow step 缺少 action".to_string())?
        .to_string();
    let mut params = resolve_templates(step, context);
    if should_skip(&params) {
        record_flow_result(context, results, &params, json!({ "skipped": true }));
        return Ok(None);
    }
    load_script_if_needed(&mut params, context)?;
    Ok(Some(PreparedFlowStep { action, params }))
}

pub(crate) fn record_flow_result(
    context: &mut Value,
    results: &mut Vec<Value>,
    params: &Value,
    result: Value,
) {
    if let Some(key) = params.get("save_as").and_then(Value::as_str) {
        context[key] = result.clone();
    }
    results.push(result);
}

pub(crate) fn find_image_result(results: &[Value]) -> Option<Value> {
    results
        .iter()
        .rev()
        .find(|item| item.get("images").is_some() || item.get("download_url").is_some())
        .cloned()
}

fn load_script_if_needed(params: &mut Value, context: &Value) -> Result<(), String> {
    if params.get("script").and_then(Value::as_str).is_some() {
        return Ok(());
    }
    let Some(scripts_dir) = context.get("scripts_dir").and_then(Value::as_str) else {
        return Ok(());
    };
    let mut combined = String::new();
    for file in collect_script_files(params) {
        let path = Path::new(scripts_dir).join(file);
        let content = std::fs::read_to_string(&path)
            .map_err(|err| format!("读取脚本失败 {}: {err}", path.display()))?;
        combined.push_str(&content);
        combined.push_str("\n;\n");
    }
    if !combined.is_empty() {
        params["script"] = Value::String(combined);
    }
    Ok(())
}
