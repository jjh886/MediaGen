use std::time::Duration;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyInit};
use aes::Aes128;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ecb::Decryptor;
use reqwest::Url;

use super::types::WeixinInboundImage;
use crate::ServerState;

const CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";

pub(crate) async fn upload_reference_image(
    state: &ServerState,
    image: &WeixinInboundImage,
) -> Result<String, String> {
    let model_id = state.store.get_weixin_preferences().default_model_id;
    let (_, model) = state
        .plugin_loader
        .resolve_model(&model_id)
        .ok_or("请先选择可用的图片模型")?;
    let caps = model
        .extra
        .get("capabilities")
        .unwrap_or(&serde_json::Value::Null);
    let limits = &caps["attachment_limits"];
    let max_bytes = limits["image_bytes"]
        .as_u64()
        .or_else(|| limits["file_bytes"].as_u64());
    let encrypted = download_cdn_image(image, max_bytes.map(|n| n.saturating_add(16))).await?;
    let plaintext = decrypt_image(state, image, encrypted)?;
    crate::protocol_attachments::validate_size(caps, "image/", plaintext.len() as u64)?;
    let media_type = validate_image(&plaintext)?;
    Ok(format!(
        "data:{media_type};base64,{}",
        STANDARD.encode(plaintext)
    ))
}

async fn download_cdn_image(
    image: &WeixinInboundImage,
    limit: Option<u64>,
) -> Result<Vec<u8>, String> {
    let url = inbound_image_url(image)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("初始化微信参考图下载失败: {error}"))?;
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| format!("下载微信参考图失败: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("下载微信参考图失败: HTTP {}", response.status()));
    }
    if response
        .content_length()
        .zip(limit)
        .is_some_and(|(n, max)| n > max)
    {
        return Err("微信参考图超过所选模型容量".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取微信参考图失败")? {
        if limit.is_some_and(|n| bytes.len().saturating_add(chunk.len()) as u64 > n) {
            return Err("微信参考图超过所选模型容量".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn inbound_image_url(image: &WeixinInboundImage) -> Result<Url, String> {
    if let Some(value) = image
        .full_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return validate_cdn_url(value);
    }
    let query = image
        .encrypt_query_param
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "微信参考图下载参数缺失".to_string())?;
    let mut url = Url::parse(&format!("{CDN_BASE_URL}/download"))
        .map_err(|error| format!("微信 CDN 地址无效: {error}"))?;
    url.query_pairs_mut()
        .append_pair("encrypted_query_param", query);
    Ok(url)
}

fn validate_cdn_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|error| format!("微信 CDN 地址无效: {error}"))?;
    super::client::validate_weixin_host(&url)?;
    Ok(url)
}

fn decrypt_image(
    state: &ServerState,
    image: &WeixinInboundImage,
    encrypted: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let Some(ciphertext) = image.aes_key_ciphertext.as_deref() else {
        return Ok(encrypted);
    };
    let encoded = state
        .credential_cipher
        .decrypt(ciphertext)
        .map_err(|_| "微信参考图密钥无法解密".to_string())?;
    let key = STANDARD
        .decode(encoded)
        .map_err(|_| "微信参考图密钥格式无效".to_string())?;
    let key: [u8; 16] = key
        .try_into()
        .map_err(|_| "微信参考图密钥长度无效".to_string())?;
    Decryptor::<Aes128>::new(&key.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&encrypted)
        .map_err(|_| "微信参考图解密失败".to_string())
}

fn validate_image(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.is_empty() {
        return Err("微信参考图大小无效".to_string());
    }
    image::load_from_memory(bytes).map_err(|_| "微信参考图格式无效".to_string())?;
    match image::guess_format(bytes).map_err(|_| "微信参考图格式无效".to_string())? {
        image::ImageFormat::Png => Ok("image/png"),
        image::ImageFormat::Jpeg => Ok("image/jpeg"),
        image::ImageFormat::WebP => Ok("image/webp"),
        _ => Err("微信参考图只支持 PNG、JPEG 或 WebP".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_trusted_weixin_cdn_urls() {
        assert!(validate_cdn_url("https://novac2c.cdn.weixin.qq.com/c2c/a").is_ok());
        assert!(validate_cdn_url("http://novac2c.cdn.weixin.qq.com/a").is_err());
        assert!(validate_cdn_url("https://example.com/a").is_err());
    }
}
