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
    let caps = model.extra.get("capabilities").unwrap_or(&Value::Null);
    if reference_urls.len() != reference_dataurls.len() {
        return Err((400, "invalid_request", "无法读取参考图片".into()));
    }
    for url in &reference_dataurls {
        crate::protocol_attachments::validate_data_url_size(caps, url)
            .map_err(|m| (400, "attachment_limit_exceeded", m))?;
    }
    let mut request = body.to_flow_request(task_id, model, reference_dataurls, vec![])?;
    let mut files = Vec::new();
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
        crate::protocol_attachments::validate_size(caps, &mime, bytes.len() as u64)
            .map_err(|m| (400, "attachment_limit_exceeded", m))?;
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
    let locals = fetch_ref_images_as_local_files(&request.ref_image_dataurls, proxy_url).await;
    files.extend(locals.iter().cloned());
    if locals.len() != reference_urls.len() {
        crate::image_assets::cleanup_temp_files(&files).await;
        return Err((400, "invalid_request", "无法保存参考图片".into()));
    }
    request.ref_image_locals = locals;
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
