use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageEndpoint {
    Generation,
    Edit,
}

impl ImageEndpoint {
    pub(crate) fn completed_event_type(self) -> &'static str {
        match self {
            Self::Generation => "image_generation.completed",
            Self::Edit => "image_edit.completed",
        }
    }
}

/// OpenAI Image API 风格请求体。
/// 生成接口接收 JSON；编辑接口同时接收 JSON 和 OpenAI SDK 默认的 multipart 文件上传。
#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct OpenAiImageRequest {
    #[serde(default)]
    pub model: String,
    pub prompt: Option<String>,
    #[serde(default)]
    pub n: Option<u32>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub output_format: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub partial_images: Option<u32>,
    #[serde(default)]
    pub output_compression: Option<u32>,
    #[serde(default)]
    pub moderation: Option<String>,
    #[serde(default)]
    pub input_fidelity: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub images: Vec<OpenAiImageRef>,
    #[serde(default)]
    pub mask: Option<OpenAiImageRef>,
    // 本地扩展字段：少量插件仍按比例选择平台选项，未传时由 size 自动映射。
    #[serde(default)]
    pub image_aspect_ratio: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
pub(crate) struct OpenAiImageRef {
    pub file_id: Option<String>,
    pub image_url: Option<String>,
}

impl OpenAiImageRequest {
    pub(crate) fn empty() -> Self {
        // multipart 解析和测试都需要同一份默认空请求，集中在这里避免字段新增时漏改。
        Self {
            model: String::new(),
            prompt: None,
            n: None,
            size: None,
            quality: None,
            response_format: None,
            output_format: None,
            background: None,
            stream: None,
            partial_images: None,
            output_compression: None,
            moderation: None,
            input_fidelity: None,
            user: None,
            images: Vec::new(),
            mask: None,
            image_aspect_ratio: None,
        }
    }

    pub(crate) fn apply_endpoint_defaults(&mut self, endpoint: ImageEndpoint) {
        if endpoint == ImageEndpoint::Edit && self.model.trim().is_empty() {
            // OpenAI 的编辑接口 model 是可选项；本地默认落到兼容别名，方便标准 SDK 调用。
            self.model = "gpt-image-1".to_string();
        }
    }

    pub(crate) fn prompt_text(&self) -> String {
        self.prompt.clone().unwrap_or_default()
    }

    pub(crate) fn output_format(&self) -> String {
        self.output_format
            .clone()
            .unwrap_or_else(|| "png".to_string())
    }

    pub(crate) fn quality(&self) -> String {
        self.quality.clone().unwrap_or_else(|| "auto".to_string())
    }

    pub(crate) fn requested_size(&self) -> String {
        self.size.clone().unwrap_or_else(|| "auto".to_string())
    }

    pub(crate) fn max_images(&self) -> u32 {
        self.n.unwrap_or(1).clamp(1, 4)
    }

    pub(crate) fn reference_urls(&self) -> Vec<String> {
        self.images
            .iter()
            .filter_map(|item| item.image_url.clone())
            .filter(|url| !url.trim().is_empty())
            .collect()
    }

    pub(crate) fn image_aspect_ratio(&self) -> String {
        if let Some(ratio) = self
            .image_aspect_ratio
            .as_ref()
            .filter(|v| !v.trim().is_empty())
        {
            return ratio.trim().to_string();
        }
        match self.size.as_deref().unwrap_or("auto") {
            // API 层保留 auto 意图；没有 Auto 选项的平台在各自插件入口兜底到 9:16。
            "auto" => "auto",
            "1536x1024" => "16:9",
            "1024x1536" => "9:16",
            "1024x1024" => "1:1",
            "8192x4096" | "6144x3072" | "4096x2048" | "2048x1024" | "2160x1080" => "2:1",
            _ => "9:16",
        }
        .to_string()
    }

    pub(crate) fn validate(
        &self,
        endpoint: ImageEndpoint,
    ) -> Result<(), (u16, &'static str, String)> {
        if self.model.trim().is_empty() {
            return Err((400, "invalid_request_error", "model 不能为空".to_string()));
        }
        if self.prompt_text().trim().is_empty() {
            return Err((400, "invalid_request_error", "prompt 不能为空".to_string()));
        }
        if self
            .response_format
            .as_deref()
            .is_some_and(|v| v != "b64_json")
        {
            return Err((
                400,
                "unsupported_parameter",
                "MediaGen 只返回 base64，请使用 response_format=b64_json 或省略该字段".to_string(),
            ));
        }
        if !matches!(self.output_format().as_str(), "png" | "jpeg" | "webp") {
            return Err((
                400,
                "invalid_request_error",
                "output_format 只能是 png、jpeg 或 webp".to_string(),
            ));
        }
        if !matches!(self.quality().as_str(), "auto" | "low" | "medium" | "high") {
            return Err((
                400,
                "invalid_request_error",
                "quality 只能是 auto、low、medium 或 high".to_string(),
            ));
        }
        if self
            .background
            .as_deref()
            .is_some_and(|v| !matches!(v, "auto" | "transparent" | "opaque"))
        {
            return Err((
                400,
                "invalid_request_error",
                "background 只能是 auto、transparent 或 opaque".to_string(),
            ));
        }
        if self
            .partial_images
            .is_some_and(|partial_images| partial_images > 3)
        {
            return Err((
                400,
                "invalid_request_error",
                "partial_images 必须在 0 到 3 之间".to_string(),
            ));
        }
        if self
            .output_compression
            .is_some_and(|compression| compression > 100)
        {
            return Err((
                400,
                "invalid_request_error",
                "output_compression 必须在 0 到 100 之间".to_string(),
            ));
        }
        if self
            .moderation
            .as_deref()
            .is_some_and(|v| !matches!(v, "auto" | "low"))
        {
            return Err((
                400,
                "invalid_request_error",
                "moderation 只能是 auto 或 low".to_string(),
            ));
        }
        if self
            .input_fidelity
            .as_deref()
            .is_some_and(|v| !matches!(v, "high" | "low"))
        {
            return Err((
                400,
                "invalid_request_error",
                "input_fidelity 只能是 high 或 low".to_string(),
            ));
        }
        if !matches!(
            self.requested_size().as_str(),
            "auto"
                | "1024x1024"
                | "1536x1024"
                | "1024x1536"
                | "8192x4096"
                | "6144x3072"
                | "4096x2048"
                | "2048x1024"
                | "2160x1080"
        ) {
            return Err((
                400,
                "invalid_request_error",
                "size 只能是 auto、1024x1024、1536x1024、1024x1536、8192x4096、6144x3072、4096x2048、2048x1024 或 2160x1080"
                    .to_string(),
            ));
        }
        if self
            .images
            .iter()
            .any(|item| item.file_id.as_ref().is_some_and(|v| !v.is_empty()))
            || self
                .mask
                .as_ref()
                .and_then(|item| item.file_id.as_ref())
                .is_some_and(|v| !v.is_empty())
        {
            return Err((
                400,
                "unsupported_parameter",
                "本地桌面端暂不提供 Files API，请使用 image_url 传入图片 URL 或 data URL"
                    .to_string(),
            ));
        }
        if endpoint == ImageEndpoint::Edit && self.reference_urls().is_empty() {
            return Err((
                400,
                "invalid_request_error",
                "图片编辑接口需要至少一个 images[].image_url".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct ImageGenContext {
    pub platform: String,
    pub model_display_name: String,
    pub flow: crate::types::FlowDef,
    pub account_id: String,
    pub profile_dir: String,
    pub account_cookies: String,
    pub request_id: String,
    pub browser_session_id: String,
    pub model_id: String,
    pub prompt: String,
    pub image_aspect_ratio: String,
    pub ref_image_local: String,
    pub ref_image_locals: Vec<String>,
    pub workspace_dir: std::path::PathBuf,
    pub initial_context: serde_json::Value,
    pub output_format: String,
    pub quality: String,
    pub size: String,
    pub output_compression: Option<u32>,
    pub background: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_request(size: &str) -> OpenAiImageRequest {
        OpenAiImageRequest {
            model: "ChatGPT".to_string(),
            prompt: Some("test".to_string()),
            size: Some(size.to_string()),
            ..OpenAiImageRequest::empty()
        }
    }

    #[test]
    fn panorama_sizes_are_valid_and_map_to_two_to_one() {
        for size in [
            "8192x4096",
            "6144x3072",
            "4096x2048",
            "2048x1024",
            "2160x1080",
        ] {
            let req = image_request(size);
            assert!(
                req.validate(ImageEndpoint::Generation).is_ok(),
                "{size} should pass"
            );
            assert_eq!(req.image_aspect_ratio(), "2:1");
        }
    }

    #[test]
    fn auto_size_preserves_auto_intent_for_plugins() {
        let req = image_request("auto");
        assert!(req.validate(ImageEndpoint::Generation).is_ok());
        assert_eq!(req.image_aspect_ratio(), "auto");
    }

    #[test]
    fn unsupported_size_still_fails() {
        let req = image_request("4096x2160");
        let err = req
            .validate(ImageEndpoint::Generation)
            .expect_err("16:9 4K should fail");
        assert_eq!(err.0, 400);
        assert!(err.2.contains("4096x2048"));
    }
}
