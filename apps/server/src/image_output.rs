use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD, Engine};
use image::{GenericImageView, ImageFormat, ImageReader};
use serde_json::{json, Value};

/// 统一验证实际文件并按客户端指定格式编码，任何下载地址都不能进入公共响应。
pub(crate) fn encode(
    images: Vec<String>,
    result: Value,
    format: &str,
    compression: Option<u32>,
    requested_size: Option<(u32, u32)>,
) -> Result<Value, String> {
    let expected = match format {
        "png" => ImageFormat::Png,
        "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        _ => return Err("不支持此图片输出格式".into()),
    };
    let mut data = Vec::new();
    let mut size = None;
    for (index, encoded) in images.into_iter().enumerate() {
        let bytes = STANDARD.decode(&encoded).map_err(|_| "上游图片编码无效")?;
        let mut reader = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|_| "上游图片格式无效")?;
        let actual = reader.format();
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let mut image = reader.decode().map_err(|_| "上游图片损坏或尺寸过大")?;
        let resized = requested_size.is_some_and(|dimensions| dimensions != image.dimensions());
        if let Some((width, height)) = requested_size.filter(|_| resized) {
            image = image.resize_to_fill(width, height, image::imageops::FilterType::Lanczos3);
        }
        let (width, height) = image.dimensions();
        size = Some(format!("{width}x{height}"));
        let encoded = if actual == Some(expected) && !resized {
            encoded
        } else {
            let mut output = Cursor::new(Vec::new());
            if expected == ImageFormat::Jpeg {
                image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut output,
                    compression.unwrap_or(90) as u8,
                )
                .encode_image(&image.to_rgb8())
                .map_err(|_| "图片编码失败")?;
            } else {
                image
                    .write_to(&mut output, expected)
                    .map_err(|_| "图片编码失败")?;
            }
            STANDARD.encode(output.into_inner())
        };
        let mut item = json!({"b64_json":encoded});
        if let Some(prompt) = result["images"][index]["revised_prompt"].as_str() {
            item["revised_prompt"] = json!(prompt);
        }
        data.push(item);
    }
    let mut response =
        json!({"created":chrono::Utc::now().timestamp(),"data":data,"output_format":format});
    if let Some(size) = size {
        response["size"] = json!(size);
    }
    for field in ["quality", "background", "usage"] {
        if let Some(value) = result.get(field).filter(|v| !v.is_null()) {
            response[field] = value.clone();
        }
    }
    Ok(response)
}

pub(crate) async fn apply_webp_compression(
    response: &mut Value,
    quality: Option<u32>,
) -> Result<(), String> {
    let Some(quality) = quality.filter(|_| response["output_format"] == "webp") else {
        return Ok(());
    };
    for item in response["data"].as_array_mut().into_iter().flatten() {
        let source = STANDARD
            .decode(item["b64_json"].as_str().ok_or("图片编码缺失")?)
            .map_err(|_| "图片编码无效")?;
        if !webp_is_lossless(&source) {
            continue;
        }
        let quality = quality.to_string();
        let bytes = crate::media_transcode::run(
            source,
            &[
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-protocol_whitelist",
                "pipe",
                "-i",
                "pipe:0",
                "-frames:v",
                "1",
                "-c:v",
                "libwebp",
                "-q:v",
                &quality,
                "-f",
                "webp",
                "pipe:1",
            ],
        )
        .await?;
        item["b64_json"] = json!(STANDARD.encode(bytes));
    }
    Ok(())
}

fn webp_is_lossless(bytes: &[u8]) -> bool {
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        if &bytes[offset..offset + 4] == b"VP8L" {
            return true;
        }
        if &bytes[offset..offset + 4] == b"VP8 " {
            return false;
        }
        let length = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        offset += 8 + length + length % 2;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_actual_encoding_and_never_media_urls() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 3)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        for format in ["png", "jpeg", "webp"] {
            let response = encode(
                vec![STANDARD.encode(png.get_ref())],
                json!({"images":[{"url":"https://private.test/image", "revised_prompt":"test"}]}),
                format,
                None,
                Some((16, 32)),
            )
            .unwrap();
            let output = STANDARD
                .decode(response["data"][0]["b64_json"].as_str().unwrap())
                .unwrap();
            assert_eq!(
                image::guess_format(&output).unwrap(),
                match format {
                    "jpeg" => ImageFormat::Jpeg,
                    "webp" => ImageFormat::WebP,
                    _ => ImageFormat::Png,
                }
            );
            assert_eq!(response["size"], "16x32");
            assert_eq!(
                image::load_from_memory(&output).unwrap().dimensions(),
                (16, 32)
            );
            assert!(!response.to_string().contains("https://"));
        }
        assert!(encode(vec!["broken".into()], json!({}), "png", None, None).is_err());
    }

    #[tokio::test]
    async fn webp_compression_survives_dimension_conversion() {
        if tokio::process::Command::new(mediagen_core::resolve_ffmpeg_binary())
            .arg("-version")
            .output()
            .await
            .is_err()
        {
            return;
        }
        let mut source = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(32, 32)
            .write_to(&mut source, ImageFormat::Png)
            .unwrap();
        let mut response = encode(
            vec![STANDARD.encode(source.get_ref())],
            json!({}),
            "webp",
            Some(50),
            Some((16, 16)),
        )
        .unwrap();
        apply_webp_compression(&mut response, Some(50))
            .await
            .unwrap();
        let bytes = STANDARD
            .decode(response["data"][0]["b64_json"].as_str().unwrap())
            .unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::WebP);
        assert!(!webp_is_lossless(&bytes));
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().dimensions(),
            (16, 16)
        );
    }
}
