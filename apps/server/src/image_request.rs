use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageEndpoint {
    Generation,
    Edit,
    Variation,
}

#[derive(Clone, Debug)]
pub struct ImageFlowRequest {
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

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Default, Deserialize)]
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
        if self.model.trim().is_empty() {
            return request_error("model 不能为空");
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
            &["auto", "low", "medium", "high"],
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
            "1536x1024" => "16:9",
            "1024x1536" => "9:16",
            "1024x1024" => "1:1",
            _ => "9:16",
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
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn applies_defaults_for_generation_and_variation() {
        let mut generation = OpenAiImageRequest::empty();
        generation.prompt = Some(Value::String("draw".to_string()));
        generation.apply_endpoint_defaults(ImageEndpoint::Generation);
        assert_eq!(generation.model, "gpt-image-1");
        assert_eq!(generation.prompt_text(), "draw");

        let mut variation = OpenAiImageRequest::empty();
        variation.images.push(OpenAiImageRef {
            image_url: Some("https://example.test/a.png".to_string()),
            file_id: None,
        });
        variation.apply_endpoint_defaults(ImageEndpoint::Variation);
        assert_eq!(variation.model, "gpt-image-1");
        assert_eq!(variation.prompt_text(), "Create a variation of the image.");
    }

    #[test]
    fn validates_required_fields_and_unsupported_inputs() {
        let empty = OpenAiImageRequest::empty();
        assert!(empty.validate(ImageEndpoint::Generation).is_err());

        let mut request = OpenAiImageRequest::empty();
        request.model = "gpt-image-1".to_string();
        request.prompt = Some(Value::String("draw".to_string()));
        request.response_format = Some("url".to_string());
        assert_eq!(
            request.validate(ImageEndpoint::Generation).unwrap_err().1,
            "unsupported_parameter"
        );

        request.response_format = Some("b64_json".to_string());
        request.output_format = Some("gif".to_string());
        assert!(request
            .validate(ImageEndpoint::Generation)
            .unwrap_err()
            .2
            .contains("output_format"));
    }

    #[test]
    fn validates_edit_reference_and_numeric_limits() {
        let mut request = OpenAiImageRequest::empty();
        request.model = "gpt-image-1".to_string();
        request.prompt = Some(Value::String("edit".to_string()));
        assert!(request.validate(ImageEndpoint::Edit).is_err());

        request.images.push(OpenAiImageRef {
            image_url: Some("https://example.test/ref.png".to_string()),
            file_id: None,
        });
        request.partial_images = Some(4);
        assert!(request
            .validate(ImageEndpoint::Edit)
            .unwrap_err()
            .2
            .contains("partial_images"));

        request.partial_images = Some(3);
        request.output_compression = Some(101);
        assert!(request
            .validate(ImageEndpoint::Edit)
            .unwrap_err()
            .2
            .contains("output_compression"));
    }

    #[test]
    fn converts_prompt_arrays_and_flow_request() {
        let request = OpenAiImageRequest {
            model: "remote-model".to_string(),
            prompt: Some(json!([{ "text": "line one" }, "line two"])),
            n: Some(9),
            size: Some("1536x1024".to_string()),
            quality: Some("high".to_string()),
            output_format: Some("webp".to_string()),
            background: Some("transparent".to_string()),
            images: vec![OpenAiImageRef {
                image_url: Some(" https://example.test/ref.png ".trim().to_string()),
                file_id: None,
            }],
            mask: Some(OpenAiImageRef {
                image_url: Some("https://example.test/mask.png".to_string()),
                file_id: None,
            }),
            ..OpenAiImageRequest::empty()
        };

        let flow = request.to_flow_request(
            "Remote Model".to_string(),
            ImageEndpoint::Edit,
            vec!["data:image/png;base64,abc".to_string()],
            Some("data:image/png;base64,mask".to_string()),
        );

        assert_eq!(flow.prompt, "line one\nline two");
        assert_eq!(flow.max_images, 4);
        assert_eq!(flow.output_format, "webp");
        assert_eq!(flow.quality, "high");
        assert_eq!(flow.background, "transparent");
        assert_eq!(flow.image_aspect_ratio, "16:9");
        assert_eq!(flow.action, "edit");
        assert_eq!(flow.ref_image_urls, vec!["https://example.test/ref.png"]);
        assert_eq!(
            flow.mask_image_url.as_deref(),
            Some("https://example.test/mask.png")
        );
        assert_eq!(
            flow.mask_image_dataurl.as_deref(),
            Some("data:image/png;base64,mask")
        );
    }

    #[test]
    fn keeps_file_id_until_api_layer_resolves_it() {
        let mut request = OpenAiImageRequest::empty();
        request.model = "gpt-image-1".to_string();
        request.prompt = Some(Value::String("draw".to_string()));
        request.images.push(OpenAiImageRef {
            file_id: Some("file-123".to_string()),
            image_url: None,
        });
        assert!(request.validate(ImageEndpoint::Generation).is_ok());

        request.images.clear();
        request.size = Some("1024x1536".to_string());
        assert_eq!(request.image_aspect_ratio(), "9:16");
        request.image_aspect_ratio = Some(" 4:3 ".to_string());
        assert_eq!(request.image_aspect_ratio(), "4:3");
    }
}
