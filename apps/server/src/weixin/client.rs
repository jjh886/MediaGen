use std::io::Cursor;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::{DynamicImage, ImageFormat, Luma};
use qrcode::QrCode;
use rand::RngCore;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::types::{GetUpdatesResponse, QrCodeResponse, QrStatusResponse, UploadUrlResponse};

const FIXED_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
const CHANNEL_VERSION: &str = "2.4.6";
const ILINK_CLIENT_VERSION: &str = "132102";

pub(crate) struct WeixinIlinkClient {
    http: reqwest::Client,
}

impl WeixinIlinkClient {
    pub(crate) fn new() -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .map_err(|error| format!("初始化微信网络客户端失败: {error}"))?;
        Ok(Self { http })
    }

    pub(crate) async fn fetch_qr_code(
        &self,
        local_tokens: Vec<String>,
    ) -> Result<QrCodeResponse, String> {
        let url = format!("{FIXED_BASE_URL}/ilink/bot/get_bot_qrcode?bot_type=3");
        self.post_json(&url, None, &json!({ "local_token_list": local_tokens }))
            .await
    }

    pub(crate) async fn poll_qr_status(
        &self,
        base_url: &str,
        qrcode: &str,
        verify_code: Option<&str>,
    ) -> Result<QrStatusResponse, String> {
        let mut url = validate_api_base_url(base_url)?
            .join("ilink/bot/get_qrcode_status")
            .map_err(|error| format!("微信扫码状态地址无效: {error}"))?;
        url.query_pairs_mut().append_pair("qrcode", qrcode);
        if let Some(code) = verify_code.filter(|value| !value.trim().is_empty()) {
            url.query_pairs_mut()
                .append_pair("verify_code", code.trim());
        }
        let response = self
            .http
            .get(url)
            .headers(common_headers(false, None)?)
            .timeout(Duration::from_secs(35))
            .send()
            .await
            .map_err(network_error)?;
        parse_response(response, "查询微信扫码状态").await
    }

    pub(crate) async fn get_updates(
        &self,
        base_url: &str,
        token: &str,
        cursor: &str,
    ) -> Result<GetUpdatesResponse, String> {
        let url = endpoint_url(base_url, "ilink/bot/getupdates")?;
        self.post_json_with_timeout(
            &url,
            Some(token),
            &json!({ "get_updates_buf": cursor, "base_info": base_info() }),
            Duration::from_secs(38),
        )
        .await
    }

    pub(crate) async fn send_text(
        &self,
        base_url: &str,
        token: &str,
        to_user_id: &str,
        context_token: &str,
        client_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let item = json!({ "type": 1, "text_item": { "text": text } });
        self.send_item(base_url, token, to_user_id, context_token, client_id, item)
            .await
    }

    pub(crate) async fn send_image(
        &self,
        base_url: &str,
        token: &str,
        to_user_id: &str,
        context_token: &str,
        client_id: &str,
        encrypted_query_param: &str,
        aes_key: &[u8; 16],
        ciphertext_size: usize,
    ) -> Result<(), String> {
        let item = image_message_item(encrypted_query_param, aes_key, ciphertext_size);
        self.send_item(base_url, token, to_user_id, context_token, client_id, item)
            .await
    }

    pub(crate) async fn get_upload_url(
        &self,
        base_url: &str,
        token: &str,
        upload: UploadRequest<'_>,
    ) -> Result<UploadUrlResponse, String> {
        let url = endpoint_url(base_url, "ilink/bot/getuploadurl")?;
        self.post_json(
            &url,
            Some(token),
            &json!({
                "filekey": upload.file_key,
                "media_type": 1,
                "to_user_id": upload.to_user_id,
                "rawsize": upload.raw_size,
                "rawfilemd5": upload.raw_md5,
                "filesize": upload.encrypted_size,
                "no_need_thumb": true,
                "aeskey": upload.aes_key_hex,
                "base_info": base_info()
            }),
        )
        .await
    }

    pub(crate) async fn upload_cdn(
        &self,
        upload_full_url: Option<&str>,
        upload_param: Option<&str>,
        file_key: &str,
        ciphertext: Vec<u8>,
    ) -> Result<String, String> {
        let url = upload_cdn_url(upload_full_url, upload_param, file_key)?;
        let response = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(ciphertext)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(format!("微信 CDN 上传失败: HTTP {}", response.status()));
        }
        response
            .headers()
            .get("x-encrypted-param")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "微信 CDN 响应缺少下载参数".to_string())
    }

    async fn send_item(
        &self,
        base_url: &str,
        token: &str,
        to_user_id: &str,
        context_token: &str,
        client_id: &str,
        item: Value,
    ) -> Result<(), String> {
        let url = endpoint_url(base_url, "ilink/bot/sendmessage")?;
        let response: Value = self
            .post_json(
                &url,
                Some(token),
                &json!({
                    "msg": {
                        "from_user_id": "",
                        "to_user_id": to_user_id,
                        "client_id": client_id,
                        "message_type": 2,
                        "message_state": 2,
                        "context_token": context_token,
                        "item_list": [item]
                    },
                    "base_info": base_info()
                }),
            )
            .await?;
        ensure_ret_ok(&response, "发送微信消息")
    }

    async fn post_json<T: DeserializeOwned>(
        &self,
        url: &str,
        token: Option<&str>,
        body: &Value,
    ) -> Result<T, String> {
        self.post_json_with_timeout(url, token, body, Duration::from_secs(15))
            .await
    }

    async fn post_json_with_timeout<T: DeserializeOwned>(
        &self,
        url: &str,
        token: Option<&str>,
        body: &Value,
        timeout: Duration,
    ) -> Result<T, String> {
        let parsed = Url::parse(url).map_err(|error| format!("微信请求地址无效: {error}"))?;
        validate_weixin_host(&parsed)?;
        let response = self
            .http
            .post(parsed)
            .headers(common_headers(true, token)?)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(network_error)?;
        parse_response(response, "微信接口请求").await
    }
}

pub(crate) struct UploadRequest<'a> {
    pub to_user_id: &'a str,
    pub file_key: &'a str,
    pub raw_size: usize,
    pub raw_md5: &'a str,
    pub encrypted_size: usize,
    pub aes_key_hex: &'a str,
}

pub(crate) fn fixed_base_url() -> &'static str {
    FIXED_BASE_URL
}

pub(crate) fn render_qr_data_url(content: &str) -> Result<String, String> {
    let code =
        QrCode::new(content.as_bytes()).map_err(|error| format!("生成二维码失败: {error}"))?;
    let image = code.render::<Luma<u8>>().min_dimensions(320, 320).build();
    let mut bytes = Vec::new();
    DynamicImage::ImageLuma8(image)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|error| format!("编码二维码失败: {error}"))?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

pub(crate) fn validate_api_base_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|error| format!("微信服务地址无效: {error}"))?;
    validate_weixin_host(&url)?;
    Ok(url)
}

pub(crate) fn validate_weixin_host(url: &Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err("微信服务地址必须使用 HTTPS".to_string());
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let trusted = host == "weixin.qq.com"
        || host.ends_with(".weixin.qq.com")
        || host == "weixin.com"
        || host.ends_with(".weixin.com");
    if !trusted {
        return Err("微信服务地址不在受信任域名内".to_string());
    }
    Ok(())
}

fn image_message_item(
    encrypted_query_param: &str,
    aes_key: &[u8; 16],
    ciphertext_size: usize,
) -> Value {
    let aes_key_hex: String = aes_key.iter().map(|byte| format!("{byte:02x}")).collect();
    json!({
        "type": 2,
        "image_item": {
            "media": {
                "encrypt_query_param": encrypted_query_param,
                "aes_key": STANDARD.encode(aes_key_hex.as_bytes()),
                "encrypt_type": 1
            },
            "mid_size": ciphertext_size
        }
    })
}

fn upload_cdn_url(
    upload_full_url: Option<&str>,
    upload_param: Option<&str>,
    file_key: &str,
) -> Result<String, String> {
    if let Some(value) = upload_full_url.filter(|value| !value.trim().is_empty()) {
        let parsed = Url::parse(value).map_err(|error| format!("微信 CDN 地址无效: {error}"))?;
        validate_weixin_host(&parsed)?;
        return Ok(parsed.to_string());
    }
    let param = upload_param.ok_or_else(|| "微信 CDN 上传参数缺失".to_string())?;
    let mut url = Url::parse(&format!("{CDN_BASE_URL}/upload"))
        .map_err(|error| format!("微信 CDN 地址无效: {error}"))?;
    url.query_pairs_mut()
        .append_pair("encrypted_query_param", param)
        .append_pair("filekey", file_key);
    Ok(url.to_string())
}

fn common_headers(include_auth_type: bool, token: Option<&str>) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert("iLink-App-Id", HeaderValue::from_static("bot"));
    headers.insert(
        "iLink-App-ClientVersion",
        HeaderValue::from_static(ILINK_CLIENT_VERSION),
    );
    if include_auth_type {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "AuthorizationType",
            HeaderValue::from_static("ilink_bot_token"),
        );
        headers.insert(
            "X-WECHAT-UIN",
            HeaderValue::from_str(&random_wechat_uin()).map_err(|error| error.to_string())?,
        );
    }
    if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
        let value = HeaderValue::from_str(&format!("Bearer {}", token.trim()))
            .map_err(|_| "微信 Token 格式无效".to_string())?;
        headers.insert(AUTHORIZATION, value);
    }
    Ok(headers)
}

fn base_info() -> Value {
    json!({
        "channel_version": CHANNEL_VERSION,
        "bot_agent": format!("MediaGen/{}", env!("CARGO_PKG_VERSION"))
    })
}

fn endpoint_url(base_url: &str, endpoint: &str) -> Result<String, String> {
    validate_api_base_url(base_url)?
        .join(endpoint)
        .map(|value| value.to_string())
        .map_err(|error| format!("微信接口地址无效: {error}"))
}

fn random_wechat_uin() -> String {
    let mut bytes = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut bytes);
    STANDARD.encode(u32::from_be_bytes(bytes).to_string())
}

async fn parse_response<T: DeserializeOwned>(
    response: reqwest::Response,
    label: &str,
) -> Result<T, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{label}失败: HTTP {status}"));
    }
    response
        .json::<T>()
        .await
        .map_err(|error| format!("{label}响应无效: {error}"))
}

fn ensure_ret_ok(response: &Value, label: &str) -> Result<(), String> {
    let ret = response.get("ret").and_then(Value::as_i64).unwrap_or(0);
    if ret == 0 {
        Ok(())
    } else {
        Err(format!("{label}失败: ret={ret}"))
    }
}

fn network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "微信网络请求超时".to_string()
    } else if error.is_connect() {
        "无法连接微信服务".to_string()
    } else {
        format!("微信网络请求失败: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_untrusted_or_non_https_hosts() {
        assert!(validate_api_base_url("http://ilinkai.weixin.qq.com").is_err());
        assert!(validate_api_base_url("https://example.com").is_err());
        assert!(validate_api_base_url(FIXED_BASE_URL).is_ok());
    }

    #[test]
    fn qr_code_is_rendered_as_png_data_url() {
        let value = render_qr_data_url("https://example.weixin.qq.com/bind?id=1").unwrap();
        assert!(value.starts_with("data:image/png;base64,"));
        assert!(value.len() > 200);
    }
}
