use std::{sync::Arc, time::Duration};

use mediagen_core::PluginManifest;
use mediagen_plugin_runtime::{run_js_step, PluginLoader};
use serde_json::Value;

use crate::flow_runtime::{
    build_image_flow_context, find_image_result, flow_result_is_error, flow_result_message,
    generate_image_flow, prepare_flow_step, record_flow_result,
};
use crate::image_request::ImageFlowRequest;
use crate::server_plugin_host::ServerPluginHost;
use crate::store::AccountRecord;

pub struct JsFlowRunner {
    plugin_loader: Arc<PluginLoader>,
}

impl JsFlowRunner {
    pub fn new(plugin_loader: Arc<PluginLoader>) -> Self {
        Self { plugin_loader }
    }

    pub async fn run_image_flow(
        &self,
        manifest: &PluginManifest,
        account: &AccountRecord,
        input: ImageFlowRequest,
        platform_config: Value,
    ) -> Result<Value, String> {
        let flow = generate_image_flow(manifest)?;
        let mut context = build_image_flow_context(
            &self.plugin_loader,
            manifest,
            account,
            input,
            platform_config,
        )?;
        let host = ServerPluginHost::new_for_account(account);
        let mut results = Vec::new();

        for step in &flow.steps {
            let Some(prepared) = prepare_flow_step(step, &mut context, &mut results)? else {
                continue;
            };
            let result = if prepared.action == "plugin.run_js" {
                run_plugin_step(&prepared.params, context.clone(), host.clone()).await?
            } else {
                return Err(format!(
                    "服务端 JS 插件暂不支持 flow action: {}",
                    prepared.action
                ));
            };
            if flow_result_is_error(&result) {
                return Err(flow_result_message(&result));
            }
            record_flow_result(&mut context, &mut results, &prepared.params, result);
        }

        find_image_result(&results).ok_or_else(|| "插件执行完成，但没有返回图片结果".to_string())
    }
}

/// 执行插件声明的步骤超时；平台只需在 manifest 中配置时长，通用执行器不硬编码平台规则。
async fn run_plugin_step(
    params: &Value,
    context: Value,
    host: ServerPluginHost,
) -> Result<Value, String> {
    let execution = run_js_step(required_str(params, "script")?, context, host);
    let Some(timeout_ms) = params.get("timeout_ms").and_then(Value::as_u64) else {
        return execution.await;
    };
    tokio::time::timeout(Duration::from_millis(timeout_ms), execution)
        .await
        .map_err(|_| format!("plugin.run_js 执行超时（{timeout_ms}ms）"))?
}

fn required_str(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("缺少 {key}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn manifest_timeout_limits_plugin_execution() {
        let host = ServerPluginHost::new_with_metadata(String::new(), &Default::default());
        let params = json!({
            "script": "function main() { host.sleep(50); return {}; }",
            "timeout_ms": 10,
        });

        let error = run_plugin_step(&params, json!({}), host).await.unwrap_err();

        assert_eq!(error, "plugin.run_js 执行超时（10ms）");
    }
}
