use actix_web::web;
use mediagen_core::{AdapterMode, ModelDef, PluginManifest};
use serde_json::Value;

use crate::flow_runner::FlowRunner;
use crate::image_assets::{fetch_ref_images_as_data_urls, fetch_ref_images_as_local_files};
use crate::js_flow_runner::JsFlowRunner;
use crate::store::AccountRecord;
use crate::video_request::{OpenAiVideoRequest, VideoFlowRequest};
use crate::ServerState;

pub(crate) async fn prepare_video_flow_request(
    body: &OpenAiVideoRequest,
    model: &ModelDef,
    task_id: String,
    proxy_url: &str,
) -> Result<(VideoFlowRequest, Vec<String>), crate::media_execution::ApiError> {
    let reference_urls = body.reference_urls();
    let reference_dataurls = fetch_ref_images_as_data_urls(&reference_urls, proxy_url).await;
    let reference_locals = fetch_ref_images_as_local_files(&reference_urls, proxy_url).await;
    let request = body.to_flow_request(
        task_id,
        model.display_name.clone(),
        reference_dataurls,
        reference_locals.clone(),
    )?;
    Ok((request, reference_locals))
}

/// Rust 只选择通用执行器；任何平台协议都由对应插件 flow 提供。
pub(crate) async fn execute_video_flow(
    state: &web::Data<ServerState>,
    manifest: &PluginManifest,
    account: &AccountRecord,
    request: VideoFlowRequest,
) -> Result<Value, String> {
    let platform_config = state
        .store
        .settings()
        .platform_generation_config(&manifest.platform_id);
    match manifest.adapter_mode {
        AdapterMode::Browser | AdapterMode::Hybrid => {
            FlowRunner::new(state.plugin_loader.clone(), state.browser_gateway.clone())
                .run_video_flow(manifest, account, request, platform_config)
                .await
        }
        AdapterMode::Api => {
            JsFlowRunner::new(state.plugin_loader.clone())
                .run_video_flow(manifest, account, request, platform_config)
                .await
        }
    }
}
