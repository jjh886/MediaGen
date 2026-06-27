use crate::engine::EngineRegistry;
use crate::error::AppError;
use crate::types::{FlowDef, StepDef, StepResult};

pub struct Orchestrator;

#[derive(Debug)]
pub struct FlowRunError {
    pub error: AppError,
    pub results: Vec<StepResult>,
}

impl Orchestrator {
    pub async fn execute_flow(
        flow: &FlowDef,
        engine: &EngineRegistry,
        initial_context: serde_json::Value,
    ) -> Result<Vec<StepResult>, AppError> {
        Self::execute_flow_with_results(flow, engine, initial_context)
            .await
            .map_err(|err| err.error)
    }

    pub async fn execute_flow_with_results(
        flow: &FlowDef,
        engine: &EngineRegistry,
        initial_context: serde_json::Value,
    ) -> Result<Vec<StepResult>, FlowRunError> {
        let mut context = initial_context;
        let mut results = Vec::new();

        for (i, step) in flow.steps.iter().enumerate() {
            if let Err(error) = execute_flow_step(i, step, engine, &mut context, &mut results).await
            {
                return Err(FlowRunError { error, results });
            }
        }

        Ok(results)
    }
}

async fn execute_flow_step(
    index: usize,
    step: &StepDef,
    engine: &EngineRegistry,
    context: &mut serde_json::Value,
    results: &mut Vec<StepResult>,
) -> Result<(), AppError> {
    let mut params = resolve_templates(&step.params, context);
    if should_skip_step(&params) {
        record_step_result(index, &params, skipped_step_result(), context, results);
        return Ok(());
    }

    load_script_content_if_needed(&mut params, context)?;
    inject_runtime_context(&step.action, &mut params, context);
    let result = match execute_step_action(&step.action, &params, context, engine).await {
        Ok(result) => result,
        Err(e) => return Err(e),
    };
    if !result.success {
        return fail_step(index, &step.action, result, results);
    }
    record_step_result(index, &params, result, context, results);
    Ok(())
}

async fn execute_step_action(
    action: &str,
    params: &serde_json::Value,
    context: &serde_json::Value,
    engine: &EngineRegistry,
) -> Result<StepResult, AppError> {
    if action == "plugin.run_js" {
        return crate::plugin_runtime::run_js_step(params, context, engine.cloned()).await;
    }
    engine.execute(action, params).await
}

fn skipped_step_result() -> StepResult {
    StepResult {
        success: true,
        data: serde_json::json!({ "skipped": true }),
        error: None,
    }
}

fn record_step_result(
    index: usize,
    params: &serde_json::Value,
    result: StepResult,
    context: &mut serde_json::Value,
    results: &mut Vec<StepResult>,
) {
    let key = params
        .get("save_as")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("step_{}", index));
    if let serde_json::Value::Object(map) = context {
        map.insert(key, result.data.clone());
    }
    results.push(result);
}

fn fail_step(
    index: usize,
    action: &str,
    result: StepResult,
    results: &mut Vec<StepResult>,
) -> Result<(), AppError> {
    let err_msg = result
        .error
        .clone()
        .unwrap_or_else(|| format!("步骤 {} ({}) 执行失败", index, action));
    results.push(result);
    Err(AppError::PlatformError(err_msg))
}

fn load_script_content_if_needed(
    params: &mut serde_json::Value,
    context: &serde_json::Value,
) -> Result<(), AppError> {
    if params.get("script").and_then(|v| v.as_str()).is_some() {
        return Ok(());
    }
    let Some(scripts_dir) = context.get("scripts_dir").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    let file_list = collect_script_file_list(params);
    if file_list.is_empty() {
        return Ok(());
    }
    let combined = read_script_files(scripts_dir, &file_list)?;
    if let serde_json::Value::Object(map) = params {
        map.insert("script".to_string(), serde_json::Value::String(combined));
    }
    Ok(())
}

fn read_script_files(scripts_dir: &str, file_list: &[String]) -> Result<String, AppError> {
    let mut combined = String::new();
    for name in file_list {
        let script_path = std::path::Path::new(scripts_dir).join(name);
        let content = std::fs::read_to_string(&script_path).map_err(|e| {
            AppError::PlatformError(format!("读取脚本 {} 失败: {e}", script_path.display()))
        })?;
        combined.push_str(&content);
        combined.push_str("\n;\n");
    }
    Ok(combined)
}

/// 从 params 收集要加载的脚本文件列表：优先 `script_files` 数组，其次 `script_file` 单值。
/// 两者都空返回 []。
fn collect_script_file_list(params: &serde_json::Value) -> Vec<String> {
    if let Some(arr) = params.get("script_files").and_then(|v| v.as_array()) {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    if let Some(name) = params.get("script_file").and_then(|v| v.as_str()) {
        return vec![name.to_string()];
    }
    Vec::new()
}

fn should_skip_step(params: &serde_json::Value) -> bool {
    if is_truthy(params.get("skip_if")) {
        return true;
    }
    if !is_truthy(params.get("skip_if_empty")) {
        return false;
    }

    ["selector", "text", "url"]
        .iter()
        .any(|key| params.get(*key).is_some_and(is_empty_like))
}

fn is_truthy(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(v)) => *v,
        Some(serde_json::Value::String(v)) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "true" | "1" | "yes")
        }
        Some(serde_json::Value::Number(v)) => v.as_i64().unwrap_or_default() != 0,
        _ => false,
    }
}

fn is_empty_like(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::String(v) => v.trim().is_empty(),
        serde_json::Value::Array(v) => v.is_empty(),
        _ => false,
    }
}

fn resolve_templates(value: &serde_json::Value, context: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            let resolved = replace_template_vars(s, context);
            serde_json::Value::String(resolved)
        }
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), resolve_templates(v, context));
            }
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            let new_arr: Vec<serde_json::Value> =
                arr.iter().map(|v| resolve_templates(v, context)).collect();
            serde_json::Value::Array(new_arr)
        }
        other => other.clone(),
    }
}

fn inject_runtime_context(
    action: &str,
    params: &mut serde_json::Value,
    context: &serde_json::Value,
) {
    let needs_context = action == "plugin.run_js"
        || action == "file.write_json"
        || params.get("body_path").is_some();
    if !needs_context {
        return;
    }

    let Some(map) = params.as_object_mut() else {
        return;
    };

    map.entry("context".to_string())
        .or_insert_with(|| context.clone());
}

fn replace_template_vars(s: &str, context: &serde_json::Value) -> String {
    let mut result = String::new();
    let mut rest = s;

    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("}}") {
            let expr = after_open[..end].trim();
            // 支持 {{js:path}} 语法：对值做 JS 字符串转义（用于 script 中的字符串嵌入）
            let (path, js_escape) = if let Some(p) = expr.strip_prefix("js:") {
                (p.trim(), true)
            } else {
                (expr, false)
            };
            if let Some(val) = resolve_path(context, path) {
                if js_escape {
                    result.push_str(&escape_js_string(&val));
                } else {
                    result.push_str(&val);
                }
            } else {
                result.push_str(&rest[start..start + 2 + end + 2]);
            }
            rest = &after_open[end + 2..];
        } else {
            result.push_str(&rest[start..]);
            rest = "";
        }
    }
    result.push_str(rest);
    result
}

/// 转义字符串使其可安全嵌入 JS 单/双引号字符串字面量
fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

fn resolve_path(context: &serde_json::Value, path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = context;

    for part in &parts {
        match current {
            serde_json::Value::Object(map) => {
                current = map.get(*part)?;
            }
            _ => return None,
        }
    }

    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null => Some("null".to_string()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests;
