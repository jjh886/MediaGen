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
    let resolved = resolve_templates(step, context);
    let mut params = action_input_params(&resolved);
    if should_skip(&params) {
        record_flow_result(context, results, &params, json!({ "skipped": true }));
        return Ok(None);
    }
    load_script_if_needed(&mut params, context)?;
    Ok(Some(PreparedFlowStep { action, params }))
}

fn action_input_params(step: &Value) -> Value {
    if let Some(input) = step.get("input") {
        return merge_control_fields(input.clone(), step);
    }
    let mut params = step.clone();
    if let Some(map) = params.as_object_mut() {
        map.remove("action");
        map.remove("_comment");
    }
    params
}

fn merge_control_fields(mut input: Value, step: &Value) -> Value {
    if !input.is_object() {
        input = json!({ "value": input });
    }
    let Some(map) = input.as_object_mut() else {
        return input;
    };
    // 这些字段属于流程控制面，不能丢，但也不要求插件重复写到 input 里。
    for key in [
        "save_as",
        "timeout_ms",
        "skip_if",
        "skip_if_empty",
        "on_error",
    ] {
        if let Some(value) = step.get(key) {
            map.entry(key.to_string()).or_insert_with(|| value.clone());
        }
    }
    input
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

pub(crate) fn flow_result_is_error(value: &Value) -> bool {
    matches!(
        value.get("status").and_then(Value::as_str),
        Some("error" | "needs_verification")
    ) || value
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|message| !message.trim().is_empty())
}

pub(crate) fn flow_result_message(value: &Value) -> String {
    let message = value
        .get("message")
        .or_else(|| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("插件执行失败")
        .to_string();
    if value.get("status").and_then(Value::as_str) == Some("needs_verification") {
        return format!("needs_verification:{message}");
    }
    message
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::{flow_result_is_error, flow_result_message, prepare_flow_step};

    #[test]
    fn script_error_field_stops_the_flow_with_its_message() {
        let result = json!({ "error": "登录已失效，请重新登录 ChatGPT" });

        assert!(flow_result_is_error(&result));
        assert_eq!(
            flow_result_message(&result),
            "登录已失效，请重新登录 ChatGPT"
        );
    }

    #[test]
    fn prepare_flow_step_uses_input_when_present() {
        let mut context = json!({ "browser": { "session_id": "session-1" } });
        let mut results = Vec::new();
        let step = json!({
            "_comment": "只给人看，不传给原子能力",
            "action": "browser.navigate",
            "input": {
                "session_id": "{{browser.session_id}}",
                "url": "https://example.test"
            },
            "save_as": "nav",
            "timeout_ms": 3000
        });

        let prepared = prepare_flow_step(&step, &mut context, &mut results)
            .unwrap()
            .unwrap();

        assert_eq!(prepared.action, "browser.navigate");
        assert_eq!(prepared.params["session_id"], "session-1");
        assert_eq!(prepared.params["url"], "https://example.test");
        assert_eq!(prepared.params["save_as"], "nav");
        assert_eq!(prepared.params["timeout_ms"], 3000);
        assert!(prepared.params.get("input").is_none());
        assert!(prepared.params.get("_comment").is_none());
    }

    #[test]
    fn prepare_flow_step_keeps_flat_legacy_params() {
        let mut context = json!({ "selector": "#send" });
        let mut results = Vec::new();
        let step = json!({
            "_comment": "旧插件仍然可以用扁平字段",
            "action": "browser.click",
            "session_id": "session-1",
            "selector": "{{selector}}",
            "save_as": "clicked"
        });

        let prepared = prepare_flow_step(&step, &mut context, &mut results)
            .unwrap()
            .unwrap();

        assert_eq!(prepared.params["session_id"], "session-1");
        assert_eq!(prepared.params["selector"], "#send");
        assert_eq!(prepared.params["save_as"], "clicked");
        assert!(prepared.params.get("action").is_none());
        assert!(prepared.params.get("_comment").is_none());
    }

    #[test]
    fn prepare_flow_step_records_skip_result() {
        let mut context = json!({});
        let mut results = Vec::new();
        let step = json!({
            "action": "browser.click",
            "selector": "",
            "skip_if_empty": true,
            "save_as": "click"
        });

        let prepared = prepare_flow_step(&step, &mut context, &mut results).unwrap();

        assert!(prepared.is_none());
        assert_eq!(context["click"]["skipped"], true);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn prepare_flow_step_loads_script_file_after_input_mapping() {
        let scripts_dir = temp_scripts_dir("single");
        fs::write(scripts_dir.join("prepare.js"), "return 1").unwrap();
        let mut context = json!({ "scripts_dir": scripts_dir.to_string_lossy() });
        let mut results = Vec::new();
        let step = json!({
            "action": "browser.run_script",
            "input": { "script_file": "prepare.js" }
        });

        let prepared = prepare_flow_step(&step, &mut context, &mut results)
            .unwrap()
            .unwrap();

        assert!(prepared.params["script"]
            .as_str()
            .unwrap()
            .contains("return 1"));
    }

    #[test]
    fn prepare_flow_step_keeps_script_file_without_scripts_dir() {
        let mut context = json!({});
        let mut results = Vec::new();
        let step = json!({
            "action": "browser.run_script",
            "script_file": "prepare.js"
        });

        let prepared = prepare_flow_step(&step, &mut context, &mut results)
            .unwrap()
            .unwrap();

        assert_eq!(prepared.params["script_file"], "prepare.js");
        assert!(prepared.params.get("script").is_none());
    }

    #[test]
    fn prepare_flow_step_wraps_non_object_input() {
        let mut context = json!({});
        let mut results = Vec::new();
        let step = json!({
            "action": "plugin.run_js",
            "input": "raw-value",
            "save_as": "raw"
        });

        let prepared = prepare_flow_step(&step, &mut context, &mut results)
            .unwrap()
            .unwrap();

        assert_eq!(prepared.params["value"], "raw-value");
        assert_eq!(prepared.params["save_as"], "raw");
    }

    fn temp_scripts_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mediagen-flow-runtime-{label}-{nanos}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
}
