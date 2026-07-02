use std::sync::Arc;
use std::time::Duration;

use mediagen_browser_gateway::{
    BrowserClipboardRequest, BrowserGateway, BrowserHistoryRequest, BrowserInputRequest,
    BrowserResizeRequest, BrowserSessionCreateRequest, BrowserSessionPurpose,
};
use mediagen_core::PluginManifest;
use mediagen_plugin_runtime::PluginLoader;
use serde_json::{json, Value};

use crate::flow_runtime::{
    build_image_flow_context, find_image_result, generate_image_flow, prepare_flow_step,
    record_flow_result,
};
use crate::image_request::ImageFlowRequest;
use crate::net_client::http_client;
use crate::store::AccountRecord;
use crate::value_utils::file_paths_from_value;

pub struct FlowRunner {
    plugin_loader: Arc<PluginLoader>,
    browser_gateway: Arc<BrowserGateway>,
}

impl FlowRunner {
    pub fn new(plugin_loader: Arc<PluginLoader>, browser_gateway: Arc<BrowserGateway>) -> Self {
        Self {
            plugin_loader,
            browser_gateway,
        }
    }

    pub async fn run_image_flow(
        &self,
        manifest: &PluginManifest,
        account: &AccountRecord,
        input: ImageFlowRequest,
    ) -> Result<Value, String> {
        if manifest.adapter_mode != mediagen_core::AdapterMode::Browser {
            return Err(format!(
                "{} 当前需要 API 插件执行器，服务端浏览器 flow 不适用",
                manifest.platform_id
            ));
        }
        let flow = generate_image_flow(manifest)?;
        let mut context = build_image_flow_context(&self.plugin_loader, manifest, account, input)?;
        let mut results = Vec::new();

        for step in &flow.steps {
            let Some(prepared) = prepare_flow_step(step, &mut context, &mut results)? else {
                continue;
            };
            let result = self
                .execute_action(&prepared.action, &prepared.params, manifest, account)
                .await?;
            if is_error_result(&result) {
                return Err(result_message(&result));
            }
            record_flow_result(&mut context, &mut results, &prepared.params, result);
        }

        find_image_result(&results).ok_or_else(|| "插件执行完成，但没有返回图片结果".to_string())
    }

    async fn execute_action(
        &self,
        action: &str,
        params: &Value,
        manifest: &PluginManifest,
        account: &AccountRecord,
    ) -> Result<Value, String> {
        match action {
            "browser.open" => self.open_browser(params, manifest, account).await,
            "browser.navigate" => self.navigate(params).await,
            "browser.history" => self.history(params).await,
            "browser.resize" => self.resize(params).await,
            "browser.input" => self.input(params).await,
            "browser.clipboard" => self.clipboard(params).await,
            "browser.cdp" => self.cdp(params).await,
            "browser.run_script" => self.run_script(params).await,
            "browser.poll_script" => self.poll_script(params).await,
            "browser.click" => self.click(params).await,
            "browser.click_text" => self.click_text(params).await,
            "browser.type_text" => self.type_text(params).await,
            "browser.set_file" => self.set_file(params).await,
            "browser.download" => self.browser_download(params).await,
            "browser.close" => self.close_browser(params).await,
            "http.download" => http_download(params, &account.proxy_url).await,
            other => Err(format!("服务端暂不支持 flow action: {other}")),
        }
    }

    async fn open_browser(
        &self,
        params: &Value,
        manifest: &PluginManifest,
        account: &AccountRecord,
    ) -> Result<Value, String> {
        let start_url = str_param(params, "restore_url")
            .or_else(|| str_param(params, "url"))
            .unwrap_or_else(|| manifest.workspace_url.clone());
        let profile_dir =
            str_param(params, "profile_dir").unwrap_or_else(|| account.profile_dir.clone());
        let mut metadata = std::collections::HashMap::new();
        if !account.proxy_url.trim().is_empty() {
            metadata.insert(
                "proxy_server".to_string(),
                Value::String(account.proxy_url.clone()),
            );
        }
        let response = self
            .browser_gateway
            .create_session(BrowserSessionCreateRequest {
                purpose: BrowserSessionPurpose::TaskTakeover,
                platform: manifest.platform_id.clone(),
                start_url,
                profile_dir: Some(profile_dir),
                headless: Some(bool_param(params, "headless").unwrap_or(true)),
                metadata,
                ..BrowserSessionCreateRequest::default()
            })
            .await
            .map_err(|err| err.to_string())?;
        Ok(json!({
            "session_id": response.session.id,
            "profile_dir": response.session.profile_dir,
            "url": response.session.url,
        }))
    }

    async fn navigate(&self, params: &Value) -> Result<Value, String> {
        let session_id = required_str(params, "session_id")?;
        let url = required_str(params, "url")?;
        let response = self
            .browser_gateway
            .navigate_session(
                &session_id,
                mediagen_browser_gateway::BrowserNavigateRequest { url },
            )
            .await
            .map_err(|err| err.to_string())?;
        sleep_ms(params.get("wait_ms").and_then(Value::as_u64)).await;
        Ok(json!({ "url": response.url }))
    }

    async fn history(&self, params: &Value) -> Result<Value, String> {
        let session_id = required_str(params, "session_id")?;
        let request: BrowserHistoryRequest =
            serde_json::from_value(params.clone()).map_err(|err| err.to_string())?;
        let response = self
            .browser_gateway
            .history_session(&session_id, request)
            .await
            .map_err(|err| err.to_string())?;
        sleep_ms(params.get("wait_ms").and_then(Value::as_u64)).await;
        serde_json::to_value(response).map_err(|err| err.to_string())
    }

    async fn resize(&self, params: &Value) -> Result<Value, String> {
        let session_id = required_str(params, "session_id")?;
        let request: BrowserResizeRequest =
            serde_json::from_value(params.clone()).map_err(|err| err.to_string())?;
        let response = self
            .browser_gateway
            .resize_session(&session_id, request)
            .await
            .map_err(|err| err.to_string())?;
        serde_json::to_value(response).map_err(|err| err.to_string())
    }

    async fn input(&self, params: &Value) -> Result<Value, String> {
        let session_id = required_str(params, "session_id")?;
        let request: BrowserInputRequest =
            serde_json::from_value(params.clone()).map_err(|err| err.to_string())?;
        let response = self
            .browser_gateway
            .dispatch_input(&session_id, request)
            .await
            .map_err(|err| err.to_string())?;
        serde_json::to_value(response).map_err(|err| err.to_string())
    }

    async fn clipboard(&self, params: &Value) -> Result<Value, String> {
        let session_id = required_str(params, "session_id")?;
        let request: BrowserClipboardRequest =
            serde_json::from_value(params.clone()).map_err(|err| err.to_string())?;
        let response = self
            .browser_gateway
            .clipboard(&session_id, request)
            .await
            .map_err(|err| err.to_string())?;
        serde_json::to_value(response).map_err(|err| err.to_string())
    }

    async fn cdp(&self, params: &Value) -> Result<Value, String> {
        let session_id = required_str(params, "session_id")?;
        let request: mediagen_browser_gateway::BrowserCdpRequest =
            serde_json::from_value(params.clone()).map_err(|err| err.to_string())?;
        let response = self
            .browser_gateway
            .send_cdp_command(&session_id, request)
            .await
            .map_err(|err| err.to_string())?;
        serde_json::to_value(response).map_err(|err| err.to_string())
    }

    async fn run_script(&self, params: &Value) -> Result<Value, String> {
        self.browser_gateway
            .evaluate_script(
                &required_str(params, "session_id")?,
                &required_str(params, "script")?,
                bool_param(params, "await_promise").unwrap_or(false),
            )
            .await
            .map_err(|err| err.to_string())
    }

    async fn poll_script(&self, params: &Value) -> Result<Value, String> {
        let deadline = std::time::Instant::now()
            + Duration::from_millis(
                params
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(600_000),
            );
        let interval = Duration::from_millis(
            params
                .get("interval_ms")
                .and_then(Value::as_u64)
                .unwrap_or(3_000),
        );
        loop {
            let result = self.run_script(params).await?;
            if result.get("status").and_then(Value::as_str) == Some("done") {
                return Ok(result);
            }
            if is_error_result(&result) {
                return Err(result_message(&result));
            }
            if std::time::Instant::now() >= deadline {
                return Err("插件轮询超时".to_string());
            }
            actix_web::rt::time::sleep(interval).await;
        }
    }

    async fn click(&self, params: &Value) -> Result<Value, String> {
        let result = self
            .browser_gateway
            .click_selector(
                &required_str(params, "session_id")?,
                &required_str(params, "selector")?,
            )
            .await
            .map_err(|err| err.to_string())?;
        sleep_ms(params.get("wait_ms").and_then(Value::as_u64)).await;
        Ok(result)
    }

    async fn click_text(&self, params: &Value) -> Result<Value, String> {
        self.browser_gateway
            .click_text(
                &required_str(params, "session_id")?,
                &required_str(params, "text")?,
                &str_param(params, "tag").unwrap_or_else(|| "*".to_string()),
                bool_param(params, "exact").unwrap_or(false),
            )
            .await
            .map_err(|err| err.to_string())
    }

    async fn type_text(&self, params: &Value) -> Result<Value, String> {
        self.browser_gateway
            .type_text(
                &required_str(params, "session_id")?,
                &required_str(params, "selector")?,
                &required_str(params, "text")?,
                bool_param(params, "clear_first").unwrap_or(false),
            )
            .await
            .map_err(|err| err.to_string())
    }

    async fn set_file(&self, params: &Value) -> Result<Value, String> {
        let files = file_paths_from_value(params.get("file_paths").unwrap_or(&Value::Null));
        self.browser_gateway
            .set_file_input(
                &required_str(params, "session_id")?,
                &required_str(params, "selector")?,
                files,
            )
            .await
            .map_err(|err| err.to_string())
    }

    async fn browser_download(&self, params: &Value) -> Result<Value, String> {
        self.browser_gateway
            .download_in_browser(
                &required_str(params, "session_id")?,
                &required_str(params, "url")?,
            )
            .await
            .map_err(|err| err.to_string())
    }

    async fn close_browser(&self, params: &Value) -> Result<Value, String> {
        let session_id = required_str(params, "session_id")?;
        let _ = self.browser_gateway.complete_session(&session_id).await;
        Ok(json!({ "closed": true }))
    }
}

async fn http_download(params: &Value, proxy_url: &str) -> Result<Value, String> {
    let url = required_str(params, "url")?;
    let response = http_client(proxy_url)?
        .get(&url)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/png")
        .to_string();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    Ok(json!({ "status": "done", "images": [{ "url": format!("data:{mime};base64,{b64}") }] }))
}

fn is_error_result(value: &Value) -> bool {
    matches!(
        value.get("status").and_then(Value::as_str),
        Some("error" | "needs_verification")
    )
}

fn result_message(value: &Value) -> String {
    value
        .get("message")
        .or_else(|| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("插件执行失败")
        .to_string()
}

fn required_str(params: &Value, key: &str) -> Result<String, String> {
    str_param(params, key).ok_or_else(|| format!("flow action 缺少 {key}"))
}

fn str_param(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn bool_param(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(Value::as_bool)
}

async fn sleep_ms(ms: Option<u64>) {
    if let Some(ms) = ms.filter(|value| *value > 0) {
        actix_web::rt::time::sleep(Duration::from_millis(ms)).await;
    }
}
