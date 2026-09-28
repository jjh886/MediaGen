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
    let mut request = body.to_flow_request(
        task_id,
        model.display_name.clone(),
        reference_dataurls,
        reference_locals.clone(),
    )?;
    let mut files = reference_locals;
    if let Some(reference) = body.extra.get("video_reference") {
        if !reference.is_object() {
            return Err((400, "invalid_request", "video_reference 必须是对象".into()));
        }
        let mut part = reference.clone();
        part["type"] = serde_json::json!("input_file");
        let (mime, bytes) = crate::protocol_attachments::decode(&part)
            .map_err(|message| (400, "invalid_request", message))?
            .ok_or((400, "invalid_request", "参考视频内容缺失".into()))?;
        if !matches!(mime.as_str(), "video/mp4" | "video/quicktime") {
            return Err((400, "invalid_request", "参考视频仅支持 MP4 或 MOV".into()));
        }
        let ext = if mime == "video/mp4" { "mp4" } else { "mov" };
        let path = std::env::temp_dir().join(format!(
            "mediagen-reference-{}.{}",
            uuid::Uuid::new_v4(),
            ext
        ));
        crate::account_login_credentials::write_private_file(&path, &bytes)
            .await
            .map_err(|_| (500, "attachment_failed", "保存参考视频失败".into()))?;
        let path = path.to_string_lossy().into_owned();
        request.ref_video_locals.push(path.clone());
        files.push(path);
        request
            .extra
            .as_object_mut()
            .map(|extra| extra.remove("video_reference"));
    }
    Ok((request, files))
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
            JsFlowRunner::new(state.plugin_loader.clone(), state.credential_cipher.clone())
                .run_video_flow(manifest, account, request, platform_config)
                .await
        }
    }
}
