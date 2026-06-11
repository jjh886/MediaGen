//! 图片接口能力校验。
//!
//! 这里只读取插件 manifest 的能力声明，不按平台名判断功能差异。

use super::image_account::PrepareError;
use super::{ImageEndpoint, OpenAiImageRequest};
use crate::types::PluginManifest;

pub(crate) fn validate_image_capabilities(
    manifest: &PluginManifest,
    endpoint: ImageEndpoint,
    body: &OpenAiImageRequest,
) -> Result<(), PrepareError> {
    let caps = &manifest.image_capabilities;
    if endpoint == ImageEndpoint::Generation && !caps.supports_generation {
        return Err((
            400,
            "unsupported_endpoint",
            format!("{} 未声明图片生成能力", manifest.platform_id),
        ));
    }
    if endpoint == ImageEndpoint::Edit && !caps.supports_edit {
        return Err((
            400,
            "unsupported_endpoint",
            format!("{} 未声明图片编辑能力", manifest.platform_id),
        ));
    }
    let has_mask = body
        .mask
        .as_ref()
        .and_then(|item| item.image_url.as_ref())
        .is_some_and(|url| !url.trim().is_empty());
    if has_mask && !caps.supports_mask {
        return Err((
            400,
            "unsupported_parameter",
            format!("{} 当前插件流程暂不支持 mask 参数", manifest.platform_id),
        ));
    }
    if let Some(max_refs) = caps.max_reference_images {
        if body.reference_urls().len() > max_refs as usize {
            return Err((
                400,
                "unsupported_parameter",
                format!("{} 最多支持 {} 张参考图", manifest.platform_id, max_refs),
            ));
        }
    }
    Ok(())
}
