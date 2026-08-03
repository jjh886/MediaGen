use std::net::IpAddr;
use std::time::Duration;

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyInit};
use aes::Aes128;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ecb::Encryptor;
use md5::{Digest, Md5};
use rand::RngCore;
use reqwest::Url;

use super::client::{UploadRequest, WeixinIlinkClient};

const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;

pub(crate) struct ImageSendContext<'a> {
    pub base_url: &'a str,
    pub bot_token: &'a str,
    pub to_user_id: &'a str,
    pub context_token: &'a str,
    pub client_id: &'a str,
    pub image_source: &'a str,
    pub workspace_dir: &'a std::path::Path,
}

pub(crate) async fn upload_and_send_image(
    client: &WeixinIlinkClient,
    context: ImageSendContext<'_>,
) -> Result<(), String> {
    let plaintext = load_image(context.image_source, context.workspace_dir).await?;
    let raw_md5 = format!("{:x}", Md5::digest(&plaintext));
    let mut aes_key = [0u8; 16];
    let mut file_key_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut aes_key);
    rand::thread_rng().fill_bytes(&mut file_key_bytes);
    let file_key = hex_bytes(&file_key_bytes);
    let aes_key_hex = hex_bytes(&aes_key);
    let ciphertext =
        Encryptor::<Aes128>::new(&aes_key.into()).encrypt_padded_vec_mut::<Pkcs7>(&plaintext);
    let ciphertext_size = ciphertext.len();
    let upload = client
        .get_upload_url(
            context.base_url,
            context.bot_token,
            UploadRequest {
                to_user_id: context.to_user_id,
                file_key: &file_key,
                raw_size: plaintext.len(),
                raw_md5: &raw_md5,
                encrypted_size: ciphertext_size,
                aes_key_hex: &aes_key_hex,
            },
        )
        .await?;
    let encrypted_query_param = client
        .upload_cdn(
            upload.upload_full_url.as_deref(),
            upload.upload_param.as_deref(),
            &file_key,
            ciphertext,
        )
        .await?;
    client
        .send_image(
            context.base_url,
            context.bot_token,
            context.to_user_id,
            context.context_token,
            context.client_id,
            &encrypted_query_param,
            &aes_key,
            ciphertext_size,
        )
        .await
}

async fn load_image(value: &str, workspace_dir: &std::path::Path) -> Result<Vec<u8>, String> {
    if value.starts_with("data:image/") {
        return decode_data_url(value);
    }
    if value.starts_with("https://") {
        return download_image(value).await;
    }
    let root = tokio::fs::canonicalize(workspace_dir)
        .await
        .map_err(|error| format!("读取图片工作区失败: {error}"))?;
    let path = tokio::fs::canonicalize(value)
        .await
        .map_err(|error| format!("读取生成图片失败: {error}"))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err("生成图片不在允许的工作区内".to_string());
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| format!("读取生成图片失败: {error}"))?;
    validate_image_bytes(&bytes)?;
    Ok(bytes)
}

fn decode_data_url(value: &str) -> Result<Vec<u8>, String> {
    let Some((meta, encoded)) = value.split_once(',') else {
        return Err("图片 data URL 格式无效".to_string());
    };
    if !meta.starts_with("data:image/") || !meta.ends_with(";base64") {
        return Err("图片 data URL 必须是 base64 图片".to_string());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "图片 base64 内容无效".to_string())?;
    validate_image_bytes(&bytes)?;
    Ok(bytes)
}

async fn download_image(value: &str) -> Result<Vec<u8>, String> {
    let client = image_download_client()?;
    let mut url = validate_image_url(value)?;
    for redirect_count in 0..=MAX_REDIRECTS {
        let response = fetch_image_response(&client, &url).await?;
        if response.status().is_redirection() {
            url = redirect_image_url(&response, &url, redirect_count)?;
            continue;
        }
        return read_image_response(response).await;
    }
    Err("下载生成图片失败".to_string())
}

fn image_download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("初始化图片下载失败: {error}"))
}

async fn fetch_image_response(
    client: &reqwest::Client,
    url: &Url,
) -> Result<reqwest::Response, String> {
    client
        .get(url.clone())
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| format!("下载生成图片失败: {error}"))
}

fn redirect_image_url(
    response: &reqwest::Response,
    current_url: &Url,
    redirect_count: usize,
) -> Result<Url, String> {
    if redirect_count == MAX_REDIRECTS {
        return Err("生成图片重定向次数过多".to_string());
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|header| header.to_str().ok())
        .ok_or_else(|| "生成图片重定向地址缺失".to_string())?;
    let joined = current_url
        .join(location)
        .map_err(|error| format!("生成图片重定向地址无效: {error}"))?;
    validate_image_url(joined.as_str())
}

async fn read_image_response(response: reqwest::Response) -> Result<Vec<u8>, String> {
    validate_image_response(&response)?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取生成图片失败: {error}"))?;
    validate_image_bytes(&bytes)?;
    Ok(bytes.to_vec())
}

fn validate_image_bytes(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err("生成图片大小无效".to_string());
    }
    image::guess_format(bytes).map_err(|_| "生成结果不是受支持的图片格式".to_string())?;
    Ok(())
}

fn validate_image_response(response: &reqwest::Response) -> Result<(), String> {
    if !response.status().is_success() {
        return Err(format!("下载生成图片失败: HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|size| size as usize > MAX_IMAGE_BYTES)
    {
        return Err("生成图片超过微信发送大小限制".to_string());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|header| header.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !content_type.is_empty() && !content_type.starts_with("image/") {
        return Err("生成结果不是有效图片".to_string());
    }
    Ok(())
}

fn validate_image_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|error| format!("生成图片地址无效: {error}"))?;
    if url.scheme() != "https" {
        return Err("生成图片地址必须使用 HTTPS".to_string());
    }
    let host = url.host_str().unwrap_or_default();
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return Err("生成图片地址不可访问".to_string());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        let blocked = match ip {
            IpAddr::V4(value) => {
                value.is_private()
                    || value.is_loopback()
                    || value.is_link_local()
                    || value.is_unspecified()
            }
            IpAddr::V6(value) => value.is_loopback() || value.is_unspecified(),
        };
        if blocked {
            return Err("生成图片地址不可访问".to_string());
        }
    }
    Ok(url)
}

fn hex_bytes(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_base64_data_url_for_queue_results() {
        let mut bytes = Vec::new();
        image::RgbImage::new(1, 1)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let data_url = format!("data:image/png;base64,{}", STANDARD.encode(bytes));
        assert!(decode_data_url(&data_url).is_ok());
    }

    #[test]
    fn rejects_local_or_non_https_image_urls() {
        assert!(validate_image_url("http://example.com/a.png").is_err());
        assert!(validate_image_url("https://127.0.0.1/a.png").is_err());
        assert!(validate_image_url("https://cdn.example.com/a.png").is_ok());
    }
}
