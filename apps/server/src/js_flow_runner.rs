use std::{sync::Arc, time::Duration};

use mediagen_core::{FlowDef, PluginManifest};
use mediagen_plugin_runtime::{run_js_step, PluginLoader};
use serde_json::Value;

use crate::flow_runtime::{
    build_image_flow_context, build_video_flow_context, find_image_result, find_video_result,
    flow_result_is_error, flow_result_message, generate_image_flow, generate_video_flow,
    prepare_flow_step, record_flow_result,
};
use crate::image_request::ImageFlowRequest;
use crate::server_plugin_host::ServerPluginHost;
use crate::store::AccountRecord;
use crate::video_request::VideoFlowRequest;

pub struct JsFlowRunner {
    plugin_loader: Arc<PluginLoader>,
    cipher: Arc<crate::generation_queue::CredentialCipher>,
}

impl JsFlowRunner {
    pub fn new(
        plugin_loader: Arc<PluginLoader>,
        cipher: Arc<crate::generation_queue::CredentialCipher>,
    ) -> Self {
        Self {
            plugin_loader,
            cipher,
        }
    }

    pub async fn run_image_flow(
        &self,
        manifest: &PluginManifest,
        account: &AccountRecord,
        input: ImageFlowRequest,
        platform_config: Value,
    ) -> Result<Value, String> {
        let flow = generate_image_flow(manifest)?;
        let context = build_image_flow_context(
            &self.plugin_loader,
            manifest,
            account,
            input,
            platform_config,
        )?;
        self.run_flow(manifest, flow, context, account, find_image_result, "图片")
            .await
    }

    pub async fn run_video_flow(
        &self,
        manifest: &PluginManifest,
        account: &AccountRecord,
        input: VideoFlowRequest,
        platform_config: Value,
    ) -> Result<Value, String> {
        let flow = generate_video_flow(manifest)?;
        let context = build_video_flow_context(
            &self.plugin_loader,
            manifest,
            account,
            input,
            platform_config,
        )?;
        self.run_flow(manifest, flow, context, account, find_video_result, "视频")
            .await
    }

    async fn run_flow(
        &self,
        manifest: &PluginManifest,
        flow: &FlowDef,
        mut context: Value,
        account: &AccountRecord,
        find_result: fn(&[Value]) -> Option<Value>,
        result_label: &str,
    ) -> Result<Value, String> {
        let host = ServerPluginHost::new_for_account(account)
            .with_storage(self.cipher.clone(), &account.profile_dir, manifest)
            .with_http_profile(manifest)
            .with_events(None, context["task_id"].as_str().unwrap_or("").to_string());
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

        find_result(&results).ok_or_else(|| format!("插件执行完成，但没有返回{result_label}结果"))
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
