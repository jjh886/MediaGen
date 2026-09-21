use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageEndpoint {
    Generation,
    Edit,
    Variation,
}

#[derive(Clone, Debug)]
pub struct ImageFlowRequest {
    /// 仅用于关联运行时资源与任务，插件不需要自行维护取消映射。
    pub task_id: Option<String>,
    pub model_id: String,
    pub model_display_name: String,
    pub prompt: String,
    pub max_images: u32,
    pub output_format: String,
    pub output_compression: Option<u32>,
    pub quality: String,
    pub background: String,
    pub partial_images: Option<u32>,
    pub moderation: Option<String>,
    pub input_fidelity: Option<String>,
    pub size: String,
    pub image_aspect_ratio: String,
    pub action: String,
    pub ref_image_locals: Vec<String>,
    pub ref_image_urls: Vec<String>,
    pub ref_image_dataurls: Vec<String>,
    pub mask_image_url: Option<String>,
    pub mask_image_dataurl: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiImageRequest {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompt: Option<Value>,
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
    #[serde(default, rename = "async")]
    pub async_generation: bool,
    /// MediaGen 扩展字段：OpenAI 图片接口没有逐请求 callback_url，这里只在 async=true 时生效。
    #[serde(default, alias = "webhook_url")]
    pub callback_url: Option<String>,
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
    #[serde(default)]
    pub image_aspect_ratio: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct OpenAiImageRef {
    pub file_id: Option<String>,
    pub image_url: Option<String>,
}

impl OpenAiImageRequest {
    pub fn empty() -> Self {
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
            async_generation: false,
            callback_url: None,
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

    pub fn apply_endpoint_defaults(&mut self, endpoint: ImageEndpoint) {
        if self.model.trim().is_empty() {
            self.model = "gpt-image-1".to_string();
        }
        if endpoint == ImageEndpoint::Variation && self.prompt.is_none() {
            self.prompt = Some(Value::String(
                "Create a variation of the image.".to_string(),
            ));
        }
    }

    pub fn validate(&self, endpoint: ImageEndpoint) -> Result<(), (u16, &'static str, String)> {
        self.output_dimensions()
            .map_err(|message| (400, "invalid_request_error", message))?;
        if self.model.trim().is_empty() {
            return request_error("model 不能为空");
        }
        if self.n.is_some_and(|n| !(1..=4).contains(&n)) {
            return request_error("n 必须在 1 到 4 之间");
        }
        if self.output_compression.is_some() && self.output_format() == "png" {
            return request_error("output_compression 只适用于 jpeg 和 webp");
        }
        if self.background() == "transparent" && self.output_format() == "jpeg" {
            return request_error("透明背景只支持 png 和 webp");
        }
        if self.partial_images.is_some_and(|n| n > 0) && self.stream != Some(true) {
            return request_error("partial_images 需要 stream=true");
        }
        if self.prompt_text().trim().is_empty() {
            return request_error("prompt 不能为空");
        }
        if endpoint != ImageEndpoint::Generation && self.reference_urls().is_empty() {
            return request_error("图片编辑/变化接口至少需要一张参考图");
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
        validate_choice(
            "output_format",
            &self.output_format(),
            &["png", "jpeg", "webp"],
        )?;
        validate_choice(
            "quality",
            &self.quality(),
            &["auto", "low", "medium", "high", "xhigh", "max"],
        )?;
        validate_choice(
            "background",
            &self.background(),
            &["auto", "transparent", "opaque"],
        )?;
        if self.partial_images.is_some_and(|value| value > 3) {
            return request_error("partial_images 必须在 0 到 3 之间");
        }
        if self.output_compression.is_some_and(|value| value > 100) {
            return request_error("output_compression 必须在 0 到 100 之间");
        }
        self.validate_callback(endpoint)?;
        Ok(())
    }

    pub fn to_flow_request(
        &self,
        model_display_name: String,
        endpoint: ImageEndpoint,
        ref_dataurls: Vec<String>,
        mask_dataurl: Option<String>,
    ) -> ImageFlowRequest {
        ImageFlowRequest {
            task_id: None,
            model_id: self.model.clone(),
            model_display_name,
            prompt: self.prompt_text(),
            max_images: self.n.unwrap_or(1).clamp(1, 4),
            output_format: self.output_format(),
            output_compression: self.output_compression,
            quality: self.quality(),
            background: self.background(),
            partial_images: self.partial_images,
            moderation: self.moderation.clone(),
            input_fidelity: self.input_fidelity.clone(),
            size: self.requested_size(),
            image_aspect_ratio: self.image_aspect_ratio(),
            action: if endpoint == ImageEndpoint::Generation {
                "generate".to_string()
            } else {
                "edit".to_string()
            },
            ref_image_locals: Vec::new(),
            ref_image_urls: self.reference_urls(),
            ref_image_dataurls: ref_dataurls,
            mask_image_url: self.mask.as_ref().and_then(|item| item.image_url.clone()),
            mask_image_dataurl: mask_dataurl,
        }
    }

    pub fn prompt_text(&self) -> String {
        match &self.prompt {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| {
                    item.get("text")
                        .and_then(Value::as_str)
                        .or_else(|| item.as_str())
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Some(value) => value.to_string(),
            None => String::new(),
        }
    }

    pub fn reference_urls(&self) -> Vec<String> {
        self.images
            .iter()
            .filter_map(|item| item.image_url.clone())
            .filter(|url| !url.trim().is_empty())
            .collect()
    }

    pub fn callback_url(&self) -> Option<String> {
        self.callback_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    fn validate_callback(
        &self,
        endpoint: ImageEndpoint,
    ) -> Result<(), (u16, &'static str, String)> {
        let Some(url) = self.callback_url() else {
            return Ok(());
        };
        if !self.async_generation || endpoint != ImageEndpoint::Generation {
            return Err((
                400,
                "unsupported_parameter",
                "callback_url 只在 /v1/images/generations 且 async=true 时生效".to_string(),
            ));
        }
        if url.len() > 2048 {
            return request_error("callback_url 长度不能超过 2048");
        }
        let parsed = reqwest::Url::parse(&url).map_err(|_| {
            (
                400,
                "invalid_request_error",
                "callback_url 格式无效".to_string(),
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return request_error("callback_url 只支持 http 或 https");
        }
        Ok(())
    }

    fn output_format(&self) -> String {
        self.output_format
            .clone()
            .unwrap_or_else(|| "png".to_string())
    }

    fn quality(&self) -> String {
        self.quality.clone().unwrap_or_else(|| "auto".to_string())
    }

    fn background(&self) -> String {
        self.background
            .clone()
            .unwrap_or_else(|| "auto".to_string())
    }

    fn requested_size(&self) -> String {
        self.size.clone().unwrap_or_else(|| "auto".to_string())
    }

    pub(crate) fn output_dimensions(&self) -> Result<Option<(u32, u32)>, String> {
        let size = self.size.as_deref().unwrap_or("auto");
        if size == "auto" {
            return Ok(None);
        }
        let dimensions = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)));
        dimensions
            .filter(|(w, h)| {
                *w > 0
                    && *h > 0
                    && *w <= 8192
                    && *h <= 8192
                    && u64::from(*w) * u64::from(*h) <= 32 * 1024 * 1024
            })
            .map(Some)
            .ok_or_else(|| "size 必须是 auto 或有效的 宽x高，且不超过 3200 万像素".into())
    }

    fn image_aspect_ratio(&self) -> String {
        if let Some(value) = self
            .image_aspect_ratio
            .as_ref()
            .filter(|v| !v.trim().is_empty())
        {
            return value.trim().to_string();
        }
        match self.requested_size().as_str() {
            "auto" => "auto",
            "1536x1024" => "3:2",
            "1024x1536" => "2:3",
            "1024x1024" => "1:1",
            _ => "auto",
        }
        .to_string()
    }
}

fn validate_choice(
    name: &'static str,
    value: &str,
    choices: &[&str],
) -> Result<(), (u16, &'static str, String)> {
    if choices.contains(&value) {
        Ok(())
    } else {
        request_error(&format!("{name} 参数不支持：{value}"))
    }
}

fn request_error<T>(message: &str) -> Result<T, (u16, &'static str, String)> {
    Err((400, "invalid_request_error", message.to_string()))
}

#[cfg(test)]
mod tests;
