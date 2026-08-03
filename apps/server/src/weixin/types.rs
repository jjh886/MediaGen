use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WeixinActionRequest {
    pub action: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub settings: Option<WeixinSettingsUpdate>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WeixinSettingsUpdate {
    pub generation_enabled: bool,
    pub direct_prompt_enabled: bool,
    pub default_model_id: String,
    pub default_aspect_ratio: String,
    pub default_max_images: u32,
    pub proactive_notifications_enabled: bool,
    pub notify_desktop_tasks: bool,
    pub notify_task_successes: bool,
    pub notify_no_accounts: bool,
    pub notify_account_actions: bool,
    #[serde(default)]
    pub quiet_hours_json: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WeixinSettingsView {
    pub status: String,
    pub masked_user_id: String,
    pub last_inbound_at: Option<String>,
    pub last_outbound_at: Option<String>,
    pub generation_enabled: bool,
    pub direct_prompt_enabled: bool,
    pub default_model_id: String,
    pub default_aspect_ratio: String,
    pub default_max_images: u32,
    pub proactive_notifications_enabled: bool,
    pub notify_desktop_tasks: bool,
    pub notify_task_successes: bool,
    pub notify_no_accounts: bool,
    pub notify_account_actions: bool,
    pub quiet_hours_json: String,
    pub models: Vec<WeixinModelView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WeixinModelView {
    pub model_id: String,
    pub display_name: String,
    pub platform: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BindingActionResult {
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qr_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct QrCodeResponse {
    pub qrcode: String,
    pub qrcode_img_content: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct QrStatusResponse {
    pub status: String,
    pub bot_token: Option<String>,
    pub ilink_bot_id: Option<String>,
    pub baseurl: Option<String>,
    pub ilink_user_id: Option<String>,
    pub redirect_host: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct GetUpdatesResponse {
    pub ret: Option<i64>,
    pub errcode: Option<i64>,
    pub errmsg: Option<String>,
    #[serde(default)]
    pub msgs: Vec<InboundMessage>,
    pub get_updates_buf: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct InboundMessage {
    pub message_id: Option<i64>,
    pub from_user_id: Option<String>,
    pub group_id: Option<String>,
    pub message_type: Option<i64>,
    pub context_token: Option<String>,
    #[serde(default)]
    pub item_list: Vec<InboundItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct InboundItem {
    #[serde(rename = "type")]
    pub item_type: Option<i64>,
    pub text_item: Option<TextItem>,
    pub image_item: Option<ImageItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct TextItem {
    pub text: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ImageItem {
    pub media: Option<CdnMedia>,
    pub thumb_media: Option<CdnMedia>,
    pub aeskey: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CdnMedia {
    pub encrypt_query_param: Option<String>,
    pub aes_key: Option<String>,
    pub full_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WeixinInboundImage {
    pub full_url: Option<String>,
    pub encrypt_query_param: Option<String>,
    pub aes_key_ciphertext: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct UploadUrlResponse {
    pub upload_param: Option<String>,
    pub upload_full_url: Option<String>,
}
