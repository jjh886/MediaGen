use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::json;
use tracing::info;

use crate::engine::AtomExecutor;
use crate::error::AppError;
use crate::types::StepResult;

pub struct WatermarkEngine;

impl WatermarkEngine {
    pub fn new() -> Self {
        Self
    }

    /// watermark.remove: 根据插件配置的策略去除水印
    ///
    /// 策略由插件 manifest 的 watermark.removal_strategy 决定：
    /// - "none" / "api_param" → 直通（无水印或 API 已处理）
    /// - "stitch"             → 图片拼接（豆包：预览图上半 + 下载图下半）
    /// - "ffmpeg"             → FFmpeg 滤镜去水印（视频）
    async fn remove(&self, params: &serde_json::Value) -> Result<StepResult, AppError> {
        let input = params["input"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("watermark.remove 缺少 input 参数".into()))?;
        let strategy = params["removal_strategy"].as_str().unwrap_or("none");

        match strategy {
            "none" | "api_param" => self.passthrough(input).await,
            "stitch" => {
                let input_b = params["input_b"].as_str().ok_or_else(|| {
                    AppError::PlatformError("stitch 策略需要 input_b 参数（第二张图片路径）".into())
                })?;
                let config = params.get("config").cloned().unwrap_or(json!({}));
                self.stitch(input, input_b, &config).await
            }
            "ffmpeg" => {
                let config = params.get("config").cloned().unwrap_or(json!({}));
                self.ffmpeg_remove(input, &config).await
            }
            _ => Err(AppError::PlatformError(format!(
                "未知的水印去除策略: {strategy}，可用: none, api_param, stitch, ffmpeg"
            ))),
        }
    }

    /// 直通：不做任何处理
    async fn passthrough(&self, input: &str) -> Result<StepResult, AppError> {
        let meta = tokio::fs::metadata(input).await?;
        info!(input, "水印直通（无需处理）");
        Ok(StepResult {
            success: true,
            data: json!({ "path": input, "size": meta.len(), "strategy_used": "passthrough" }),
            error: None,
        })
    }

    /// 图片拼接去水印
    ///
    /// 原理：两张图片的水印分别在不同位置，取各自干净的部分拼接
    /// 默认：input（预览图）取上半部分 + input_b（下载图）取下半部分
    ///
    /// config 可选参数：
    /// - top_source: "input" | "input_b"（哪张图取上半，默认 "input"）
    /// - split_ratio: 0.5（分割比例，默认 50%）
    async fn stitch(
        &self,
        input: &str,
        input_b: &str,
        config: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let input_a = PathBuf::from(input);
        let input_b = PathBuf::from(input_b);
        let top_source = config["top_source"].as_str().unwrap_or("input").to_string();
        let split_ratio = config["split_ratio"].as_f64().unwrap_or(0.5) as f32;

        let result = tokio::task::spawn_blocking(move || {
            do_stitch(&input_a, &input_b, &top_source, split_ratio)
        })
        .await
        .map_err(|e| AppError::PlatformError(format!("拼接任务失败: {e}")))?;

        let (path, size) = result?;
        info!(path = %path, "图片拼接去水印完成");

        Ok(StepResult {
            success: true,
            data: json!({ "path": path, "size": size, "strategy_used": "stitch" }),
            error: None,
        })
    }

    /// FFmpeg 去水印（视频）
    async fn ffmpeg_remove(
        &self,
        input: &str,
        config: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        let filters = config["ffmpeg_filters"]
            .as_str()
            .ok_or_else(|| AppError::PlatformError("ffmpeg 策略需要 ffmpeg_filters 配置".into()))?;

        let input_path = PathBuf::from(input);
        let stem = input_path.file_stem().unwrap_or_default().to_string_lossy();
        let ext = input_path.extension().unwrap_or_default().to_string_lossy();
        let output_path = input_path.with_file_name(format!("{stem}_clean.{ext}"));

        use crate::process_ext::NoWindowExt;
        let output = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                input,
                "-vf",
                filters,
                "-c:a",
                "copy",
                &output_path.to_string_lossy(),
            ])
            .no_window()
            .output()
            .await
            .map_err(|e| AppError::PlatformError(format!("FFmpeg 执行失败: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::PlatformError(format!(
                "FFmpeg 处理失败: {stderr}"
            )));
        }

        let meta = tokio::fs::metadata(&output_path).await?;
        info!(path = %output_path.display(), "FFmpeg 水印去除完成");

        Ok(StepResult {
            success: true,
            data: json!({
                "path": output_path.to_string_lossy(),
                "size": meta.len(),
                "strategy_used": "ffmpeg"
            }),
            error: None,
        })
    }
}

/// 图片拼接（CPU 密集型，在阻塞线程运行）
fn do_stitch(
    input_a: &std::path::Path,
    input_b: &std::path::Path,
    top_source: &str,
    split_ratio: f32,
) -> Result<(String, u64), AppError> {
    use image::{GenericImageView, ImageBuffer, Rgba};

    let img_a = image::open(input_a)
        .map_err(|e| AppError::PlatformError(format!("打开图片 A 失败: {e}")))?;
    let img_b = image::open(input_b)
        .map_err(|e| AppError::PlatformError(format!("打开图片 B 失败: {e}")))?;

    let (w_a, h_a) = img_a.dimensions();
    let (w_b, h_b) = img_b.dimensions();

    // 使用较小的宽度，高度取 A 的高度（两图应该同尺寸）
    let w = w_a.min(w_b);
    let h = h_a;

    let split_y = (h as f32 * split_ratio) as u32;

    // 确定上下来源
    let (top_img, bottom_img) = if top_source == "input_b" {
        (&img_b, &img_a)
    } else {
        (&img_a, &img_b)
    };

    let mut output: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(w, h);

    // 上半部分
    for y in 0..split_y {
        for x in 0..w {
            output.put_pixel(x, y, top_img.get_pixel(x, y));
        }
    }

    // 下半部分
    for y in split_y..h {
        for x in 0..w {
            if x < w_b && y < h_b {
                output.put_pixel(x, y, bottom_img.get_pixel(x, y));
            }
        }
    }

    let stem = input_a.file_stem().unwrap_or_default().to_string_lossy();
    let out_path = input_a.with_file_name(format!("{stem}_clean.png"));

    output
        .save(&out_path)
        .map_err(|e| AppError::PlatformError(format!("保存拼接图片失败: {e}")))?;

    let size = std::fs::metadata(&out_path)?.len();
    Ok((out_path.to_string_lossy().to_string(), size))
}

#[async_trait]
impl AtomExecutor for WatermarkEngine {
    fn handles(&self, action_prefix: &str) -> bool {
        action_prefix == "watermark"
    }

    async fn execute(
        &self,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<StepResult, AppError> {
        match action {
            "watermark.remove" => self.remove(params).await,
            _ => Err(AppError::PlatformError(format!(
                "未知的 watermark 操作: {action}，可用: watermark.remove"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_prefix() {
        let engine = WatermarkEngine::new();
        assert!(engine.handles("watermark"));
        assert!(!engine.handles("image"));
    }
}
