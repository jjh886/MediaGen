use actix_web::web;
use mediagen_core::{AdapterMode, ModelDef, PluginManifest};
use serde_json::Value;

use crate::flow_runner::FlowRunner;
use crate::image_assets::{
    fetch_optional_data_url, fetch_ref_images_as_data_urls, fetch_ref_images_as_local_files,
};
use crate::image_request::{ImageEndpoint, ImageFlowRequest, OpenAiImageRequest};
use crate::js_flow_runner::JsFlowRunner;
use crate::store::AccountRecord;
use crate::ServerState;

pub(crate) async fn prepare_flow_request(
    body: &OpenAiImageRequest,
    model: &ModelDef,
    endpoint: ImageEndpoint,
    proxy_url: &str,
) -> Result<(ImageFlowRequest, Vec<String>), String> {
    let ref_urls = body.reference_urls();
    let ref_dataurls = fetch_ref_images_as_data_urls(&ref_urls, proxy_url).await;
    if ref_dataurls.len() != ref_urls.len() {
        return Err("invalid_input: 无法读取参考图片".into());
    }
    let ref_image_locals = fetch_ref_images_as_local_files(&ref_urls, proxy_url).await;
    if ref_image_locals.len() != ref_urls.len() {
        crate::image_assets::cleanup_temp_files(&ref_image_locals).await;
        return Err("invalid_input: 无法读取参考图片".into());
    }
    let mask_dataurl = fetch_optional_data_url(
        body.mask
            .as_ref()
            .and_then(|item| item.image_url.as_deref()),
        proxy_url,
    )
    .await;
    if body.mask.is_some() && mask_dataurl.is_none() {
        crate::image_assets::cleanup_temp_files(&ref_image_locals).await;
        return Err("invalid_input: 无法读取蒙版图片".into());
    }
    let mut flow_request = body.to_flow_request(
        model.display_name.clone(),
        endpoint,
        ref_dataurls,
        mask_dataurl,
    );
    flow_request.ref_image_locals = ref_image_locals.clone();
    Ok((flow_request, ref_image_locals))
}

/// Rust 只选择通用执行器，具体平台步骤仍全部来自插件 manifest。
pub(crate) async fn execute_image_flow(
    state: &web::Data<ServerState>,
    manifest: &PluginManifest,
    account: &AccountRecord,
    flow_request: ImageFlowRequest,
) -> Result<Value, String> {
    let platform_config = state
        .store
        .settings()
        .platform_generation_config(&manifest.platform_id);
    match manifest.adapter_mode {
        AdapterMode::Browser | AdapterMode::Hybrid => {
            FlowRunner::new(state.plugin_loader.clone(), state.browser_gateway.clone())
                .run_image_flow(manifest, account, flow_request, platform_config)
                .await
        }
        AdapterMode::Api => {
            JsFlowRunner::new(state.plugin_loader.clone(), state.credential_cipher.clone())
                .run_image_flow(manifest, account, flow_request, platform_config)
                .await
        }
    }
}
