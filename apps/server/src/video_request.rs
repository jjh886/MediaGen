use mediagen_core::PluginManifest;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::image_request::OpenAiImageRef;
use crate::media_execution::ApiError;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenAiVideoRequest {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub input_reference: Option<OpenAiImageRef>,
    /// MediaGen 扩展：部分平台支持首尾帧或多图参考，标准单图字段仍优先兼容。
    #[serde(default)]
    pub input_references: Vec<OpenAiImageRef>,
    #[serde(default)]
    pub seconds: Option<Value>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug)]
pub struct VideoFlowRequest {
    pub ref_video_locals: Vec<String>,
    pub task_id: String,
    pub model_id: String,
    pub model_display_name: String,
    pub prompt: String,
    pub seconds: String,
    pub size: String,
    pub quality: String,
    pub aspect_ratio: String,
    pub ref_image_urls: Vec<String>,
    pub ref_image_dataurls: Vec<String>,
    pub ref_image_locals: Vec<String>,
    pub extra: Value,
}

impl OpenAiVideoRequest {
    pub fn empty() -> Self {
        Self {
            model: String::new(),
            prompt: String::new(),
            input_reference: None,
            input_references: Vec::new(),
            seconds: None,
            size: None,
            quality: None,
            user: None,
            extra: Map::new(),
        }
    }

    pub fn validate(&self) -> Result<(), ApiError> {
        if self.extra.contains_key("ref_video_locals")
            || self.extra.contains_key("ref_audio_locals")
        {
            return request_error(
                "不能提交服务器本地路径，请使用 video_reference.file_id 或 data URL",
            );
        }
        if self.model.trim().is_empty() {
            return request_error("model 不能为空");
        }
        if self.prompt.trim().is_empty() {
            return request_error("prompt 不能为空");
        }
        if self.prompt.chars().count() > 32_000 {
            return request_error("prompt 不能超过 32000 个字符");
        }
        let seconds = self.seconds_text()?;
        if seconds.parse::<u32>().is_err() {
            return request_error("seconds 必须是正整数或数字字符串");
        }
        if self.size_dimensions().is_none() {
            return request_error("size 必须使用 宽x高 格式，例如 1280x720");
        }
        Ok(())
    }

    pub fn apply_manifest_defaults(&mut self, manifest: &PluginManifest) {
        let Some(capabilities) = manifest.extra.get("video_capabilities") else {
            return;
        };
        if self.seconds.is_none() {
            if let Some(seconds) = capabilities
                .get("default_duration_seconds")
                .and_then(Value::as_u64)
            {
                self.seconds = Some(Value::String(seconds.to_string()));
            }
        }
        if self
            .size
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            if let Some(size) = capabilities.get("default_size").and_then(Value::as_str) {
                self.size = Some(size.to_string());
            }
        }
    }

    pub fn seconds_text(&self) -> Result<String, ApiError> {
        let value = match self.seconds.as_ref() {
            None => "4".to_string(),
            Some(Value::String(value)) => value.trim().to_string(),
            Some(Value::Number(value)) => value.to_string(),
            Some(_) => return request_error("seconds 必须是数字字符串"),
        };
        let parsed = value
            .parse::<u32>()
            .map_err(|_| invalid_request("seconds 必须是正整数或数字字符串"))?;
        if parsed == 0 || parsed > 60 {
            return request_error("seconds 必须在 1 到 60 之间");
        }
        Ok(parsed.to_string())
    }

    pub fn size_text(&self) -> String {
        self.size
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("720x1280")
            .to_string()
    }

    pub fn quality_text(&self) -> String {
        self.quality
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("standard")
            .to_string()
    }

    pub fn reference_urls(&self) -> Vec<String> {
        self.input_reference
            .iter()
            .chain(self.input_references.iter())
            .filter_map(|item| item.image_url.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn size_dimensions(&self) -> Option<(u32, u32)> {
        let size = self.size_text();
        let (width, height) = size.split_once('x')?;
        let width = width.parse::<u32>().ok().filter(|value| *value > 0)?;
        let height = height.parse::<u32>().ok().filter(|value| *value > 0)?;
        Some((width, height))
    }

    pub fn aspect_ratio(&self) -> String {
        let Some((width, height)) = self.size_dimensions() else {
            return "9:16".to_string();
        };
        let (mut divisor, mut remainder) = (width, height);
        while remainder != 0 {
            (divisor, remainder) = (remainder, divisor % remainder);
        }
        let ratio = (width / divisor, height / divisor);
        if ratio == (7, 3) {
            "21:9".to_string()
        } else {
            format!("{}:{}", ratio.0, ratio.1)
        }
    }

    pub fn to_flow_request(
        &self,
        task_id: String,
        model_display_name: String,
        ref_image_dataurls: Vec<String>,
        ref_image_locals: Vec<String>,
    ) -> Result<VideoFlowRequest, ApiError> {
        Ok(VideoFlowRequest {
            ref_video_locals: Vec::new(),
            task_id,
            model_id: self.model.clone(),
            model_display_name,
            prompt: self.prompt.clone(),
            seconds: self.seconds_text()?,
            size: self.size_text(),
            quality: self.quality_text(),
            aspect_ratio: self.aspect_ratio(),
            ref_image_urls: self.reference_urls(),
            ref_image_dataurls,
            ref_image_locals,
            extra: Value::Object(self.extra.clone()),
        })
    }
}

pub(crate) fn validate_video_capabilities(
    manifest: &PluginManifest,
    request: &OpenAiVideoRequest,
) -> Result<(), ApiError> {
    let Some(capabilities) = manifest.extra.get("video_capabilities") else {
        return request_error("插件没有声明视频生成能力");
    };
    if capabilities
        .get("supports_generation")
        .and_then(Value::as_bool)
        == Some(false)
    {
        return request_error("当前插件不支持视频生成");
    }
    let reference_count = request.reference_urls().len();
    if reference_count > 0
        && capabilities
            .get("supports_image_reference")
            .and_then(Value::as_bool)
            == Some(false)
    {
        return request_error("当前视频模型不支持参考图");
    }
    let max_references = capabilities
        .get("max_reference_images")
        .and_then(Value::as_u64)
        .unwrap_or(1) as usize;
    if reference_count > max_references {
        return request_error(&format!("参考图不能超过 {max_references} 张"));
    }
    let seconds = request
        .seconds_text()?
        .parse::<u64>()
        .map_err(|_| invalid_request("seconds 无效"))?;
    let min_seconds = capabilities
        .get("min_duration_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let max_seconds = capabilities
        .get("max_duration_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(60);
    if seconds < min_seconds || seconds > max_seconds {
        return request_error(&format!(
            "当前模型只支持 {min_seconds}-{max_seconds} 秒视频"
        ));
    }
    validate_declared_choice(
        capabilities.get("supported_seconds"),
        &seconds.to_string(),
        "视频时长",
    )?;
    validate_declared_choice(
        capabilities.get("supported_sizes"),
        &request.size_text(),
        "size",
    )?;
    validate_declared_choice(
        capabilities.get("supported_aspect_ratios"),
        &request.aspect_ratio(),
        "视频比例",
    )?;
    Ok(())
}

fn validate_declared_choice(
    choices: Option<&Value>,
    selected: &str,
    label: &str,
) -> Result<(), ApiError> {
    let Some(choices) = choices
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
    else {
        return Ok(());
    };
    if choices.iter().any(|value| {
        value.as_str() == Some(selected)
            || value
                .as_u64()
                .is_some_and(|number| number.to_string() == selected)
    }) {
        Ok(())
    } else {
        request_error(&format!("当前模型不支持{label} {selected}"))
    }
}

fn request_error<T>(message: &str) -> Result<T, ApiError> {
    Err(invalid_request(message))
}

fn invalid_request(message: &str) -> ApiError {
    (400, "invalid_request_error", message.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn public_video_requests_cannot_read_server_local_paths() {
        for field in ["ref_video_locals", "ref_audio_locals"] {
            let mut request = OpenAiVideoRequest::empty();
            request.model = "video".into();
            request.prompt = "测试".into();
            request
                .extra
                .insert(field.into(), json!(["/private/credentials"]));
            assert!(request.validate().is_err());
        }
    }

    #[test]
    fn accepts_openai_string_seconds_and_maps_size_to_ratio() {
        let request: OpenAiVideoRequest = serde_json::from_value(json!({
            "model": "video-model",
            "prompt": "a cat",
            "seconds": "8",
            "size": "1280x720"
        }))
        .unwrap();

        assert!(request.validate().is_ok());
        assert_eq!(request.seconds_text().unwrap(), "8");
        assert_eq!(request.aspect_ratio(), "16:9");
    }

    #[test]
    fn preserves_non_widescreen_ratios_and_rejects_invalid_dimensions() {
        let mut request = OpenAiVideoRequest::empty();
        request.model = "video-model".into();
        request.prompt = "move".into();
        for (size, ratio) in [
            ("960x1280", "3:4"),
            ("1280x960", "4:3"),
            ("1680x720", "21:9"),
            ("1024x1024", "1:1"),
            ("720x1280", "9:16"),
        ] {
            request.size = Some(size.into());
            assert!(request.validate().is_ok());
            assert_eq!(request.aspect_ratio(), ratio);
        }
        for size in ["0x720", "1280x0", "ax720", "1280x720x1", "-1x720"] {
            request.size = Some(size.into());
            assert!(request.validate().is_err(), "{size}");
        }
    }

    #[test]
    fn rejects_empty_prompt_and_invalid_duration() {
        let mut request = OpenAiVideoRequest::empty();
        request.model = "video-model".to_string();
        assert!(request.validate().is_err());

        request.prompt = "move".to_string();
        request.seconds = Some(json!(0));
        assert!(request.validate().is_err());
    }

    #[test]
    fn combines_standard_and_extended_reference_images() {
        let request: OpenAiVideoRequest = serde_json::from_value(json!({
            "model": "video-model",
            "prompt": "move",
            "input_reference": { "image_url": "https://example.test/first.png" },
            "input_references": [{ "image_url": "https://example.test/last.png" }]
        }))
        .unwrap();

        assert_eq!(request.reference_urls().len(), 2);
    }

    #[test]
    fn manifest_capabilities_limit_duration_and_references() {
        let manifest: PluginManifest = serde_json::from_value(json!({
            "platform_id": "test",
            "display_name": "Test",
            "video_capabilities": {
                "supports_generation": true,
                "supports_image_reference": true,
                "max_reference_images": 1,
                "min_duration_seconds": 5,
                "max_duration_seconds": 30,
                "supported_aspect_ratios": ["16:9"]
            }
        }))
        .unwrap();
        let mut request = OpenAiVideoRequest::empty();
        request.model = "video".to_string();
        request.prompt = "move".to_string();
        request.seconds = Some(json!(4));
        request.size = Some("1280x720".to_string());

        assert!(validate_video_capabilities(&manifest, &request).is_err());

        request.seconds = Some(json!(5));
        assert!(validate_video_capabilities(&manifest, &request).is_ok());
    }

    #[test]
    fn manifest_can_override_openai_defaults_for_platform() {
        let manifest: PluginManifest = serde_json::from_value(json!({
            "platform_id": "test",
            "display_name": "Test",
            "video_capabilities": {
                "default_duration_seconds": 5,
                "default_size": "1280x720"
            }
        }))
        .unwrap();
        let mut request = OpenAiVideoRequest::empty();

        request.apply_manifest_defaults(&manifest);

        assert_eq!(request.seconds_text().unwrap(), "5");
        assert_eq!(request.size_text(), "1280x720");
    }
}
