use std::time::Duration;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyInit};
use aes::Aes128;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ecb::Decryptor;
use reqwest::Url;

use super::types::WeixinInboundImage;
use crate::ServerState;

const CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
const MAX_REFERENCE_BYTES: usize = 20 * 1024 * 1024;

pub(crate) async fn upload_reference_image(
    state: &ServerState,
    image: &WeixinInboundImage,
) -> Result<String, String> {
    let encrypted = download_cdn_image(image).await?;
    let plaintext = decrypt_image(state, image, encrypted)?;
    let media_type = validate_image(&plaintext)?;
    Ok(format!(
        "data:{media_type};base64,{}",
        STANDARD.encode(plaintext)
    ))
}

async fn download_cdn_image(image: &WeixinInboundImage) -> Result<Vec<u8>, String> {
    let url = inbound_image_url(image)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("初始化微信参考图下载失败: {error}"))?;
    let response = client
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
        .is_some_and(|size| size as usize > MAX_REFERENCE_BYTES + 32)
    {
        return Err("微信参考图超过 20MB".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取微信参考图失败: {error}"))?;
    if bytes.is_empty() || bytes.len() > MAX_REFERENCE_BYTES + 32 {
        return Err("微信参考图大小无效".to_string());
    }
    Ok(bytes.to_vec())
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
    if bytes.is_empty() || bytes.len() > MAX_REFERENCE_BYTES {
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
