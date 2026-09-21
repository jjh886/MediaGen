use crate::{
    protocol_api::{error, run_with_failover},
    ServerState,
};
use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse};
use base64::prelude::{Engine as _, BASE64_STANDARD};
use futures_util::StreamExt;
use serde_json::{json, Value};

pub(crate) async fn transcribe(
    state: web::Data<ServerState>,
    request: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    if !crate::routes::api_key_allowed(&state, &request) {
        return error(401, "invalid_api_key", "API Key 无效");
    }
    let body = match multipart(&request, payload, 25 * 1024 * 1024).await {
        Ok(value) => value,
        Err(message) => return error(400, "invalid_request", &message),
    };
    let Some(id) = body["model"].as_str() else {
        return error(400, "missing_model", "缺少 model");
    };
    if body["file"]["base64"]
        .as_str()
        .is_none_or(|value| value.is_empty())
    {
        return error(400, "missing_file", "缺少音频文件");
    }
    let key = crate::routes::api_key_from_request(&request).unwrap_or_default();
    let Some((manifest, model, _)) = crate::model_api::resolve(&state, id, &key) else {
        return error(404, "model_not_found", "模型不存在");
    };
    if model.task_kind != mediagen_core::TaskKind::AudioTranscription {
        return error(400, "unsupported_endpoint", "模型不支持音频转写");
    }
    if let Err(message) = crate::protocol_request::validate_capabilities(
        &body,
        model.extra.get("capabilities").unwrap_or(&Value::Null),
    ) {
        return error(400, "unsupported_parameter", &message);
    }
    let flow = if request.path().ends_with("/translations") {
        "translate_audio"
    } else {
        "transcribe_audio"
    };
    if !manifest.flows.contains_key(flow) {
        return error(400, "unsupported_endpoint", "模型不支持此音频接口");
    }
    let task_id = crate::request_identity::id(&request);
    let (mut account, _lease) = match crate::protocol_task::reserve(
        &state,
        &manifest,
        id,
        "audio_transcription",
        &key,
        &task_id,
        Some(&body),
    ) {
        Ok(value) => value,
        Err(_) => return error(429, "account_unavailable", "没有可用账号或账号代理不可用"),
    };
    let format = body["response_format"]
        .as_str()
        .unwrap_or("json")
        .to_string();
    let execution = run_with_failover(
        &state,
        &manifest,
        &mut account,
        flow,
        body,
        None,
        &task_id,
        Value::Null,
        &key,
    );
    let result = tokio::select! {
        result = execution => result,
        _ = crate::protocol_task::cancelled(&state.store, &task_id) => return error(409, "task_cancelled", "请求已取消"),
    };
    match result {
        Ok(result) if result["text"].is_string() => {
            state.store.mark_account_usage(&account.id);
            state
                .store
                .complete_task(&task_id, json!({"status":"completed"}));
            if format == "text" {
                HttpResponse::Ok()
                    .content_type("text/plain; charset=utf-8")
                    .body(result["text"].as_str().unwrap_or("").to_string())
            } else {
                HttpResponse::Ok().json(json!({"text":result["text"]}))
            }
        }
        Ok(_) => error(502, "invalid_transcription", "上游未返回有效转写文本"),
        Err(message) => {
            let context = crate::media_execution::MediaFlowContext {
                manifest: &manifest,
                model: &model,
                account: account.clone(),
                task_id: task_id.clone(),
            };
            crate::media_execution::classify_and_apply_failure(&state, &context, &message);
            state.store.fail_task(&task_id, "音频处理失败".into());
            let (status, code, public) = crate::protocol_api::public_failure(&message);
            error(status, code, &public)
        }
    }
}

pub(crate) async fn multipart(
    request: &HttpRequest,
    payload: web::Payload,
    limit: usize,
) -> Result<Value, String> {
    if !request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("multipart/form-data"))
    {
        return Err("此接口要求 multipart/form-data".into());
    }
    let mut parts = Multipart::new(request.headers(), payload);
    let mut result = json!({});
    let mut total = 0;
    while let Some(part) = parts.next().await {
        let mut part = part.map_err(|_| "multipart 格式无效")?;
        let name = part.name().ok_or("字段缺少名称")?.to_string();
        if result.get(&name).is_some() {
            return Err("请求包含重复字段".into());
        }
        let filename = part
            .content_disposition()
            .and_then(|value| value.get_filename())
            .map(|value| {
                value
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("audio")
                    .to_string()
            });
        let mime = part
            .content_type()
            .map(ToString::to_string)
            .unwrap_or("application/octet-stream".into());
        let mut bytes = Vec::new();
        while let Some(chunk) = part.next().await {
            let chunk = chunk.map_err(|_| "读取上传内容失败")?;
            total += chunk.len();
            if total > limit {
                return Err("上传内容超过大小限制".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        result[&name] = if let Some(filename) = filename {
            json!({"base64":BASE64_STANDARD.encode(bytes),"filename":filename,"mime_type":mime})
        } else {
            json!(String::from_utf8(bytes).map_err(|_| "表单字段不是 UTF-8")?)
        };
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test as at, App};
    fn state() -> web::Data<ServerState> {
        let root = std::env::temp_dir().join(format!("audio-contract-{}", uuid::Uuid::new_v4()));
        let dir = root.join("plugins/test/scripts");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("transcribe.js"), "function main(ctx){if(ctx.request.prompt==='fail')throw new Error('LOGIN_EXPIRED');if(ctx.request.prompt==='slow')host.sleep(5000);return {text:ctx.request.language||'transcribed'}}").unwrap();
        std::fs::write(root.join("plugins/test/manifest.json"), json!({"platform_id":"test","display_name":"Test","adapter_mode":"api","models":[{"model_id":"audio-test","display_name":"Audio","task_kind":"audio_transcription"}],"flows":{"transcribe_audio":{"steps":[{"action":"plugin.run_js","script_file":"transcribe.js"}]},"translate_audio":{"steps":[{"action":"plugin.run_js","script_file":"transcribe.js"}]}},"failure_strategy":[{"contains_any":["LOGIN_EXPIRED"],"category":"login_expired","account_state":"login_expired","retry_action":"failover"}]}).to_string()).unwrap();
        let loader =
            mediagen_plugin_runtime::PluginLoader::load_from(&root.join("plugins")).unwrap();
        let state = crate::test_fixtures::server_state(root, loader);
        state.store.update_settings(crate::store::SettingsUpdate {
            api_keys: Some("key".into()),
            ..Default::default()
        });
        state
            .store
            .insert_account(crate::test_fixtures::account_record(
                "audio-account",
                "test",
                "Audio",
                "",
            ));
        web::Data::new(state)
    }
    fn form(extra: &str) -> String {
        format!("--boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\naudio-test\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFFtest\r\n{extra}--boundary--\r\n")
    }
    #[actix_web::test]
    async fn multipart_transcription_translation_and_errors_follow_contract() {
        let state = state();
        let app = at::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        for (path, extra, expected) in [
            ("transcriptions", "", "transcribed"),
            (
                "translations",
                "--boundary\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nen\r\n",
                "en",
            ),
        ] {
            let response = at::call_service(
                &app,
                at::TestRequest::post()
                    .uri(&format!("/v1/audio/{path}"))
                    .insert_header(("authorization", "Bearer key"))
                    .insert_header(("content-type", "multipart/form-data; boundary=boundary"))
                    .set_payload(form(extra))
                    .to_request(),
            )
            .await;
            assert_eq!(response.status(), 200);
            let result: Value = at::read_body_json(response).await;
            assert_eq!(result["text"], expected);
        }
        let response = at::call_service(&app, at::TestRequest::post().uri("/v1/audio/transcriptions").insert_header(("authorization","Bearer key")).insert_header(("content-type","multipart/form-data; boundary=boundary")).set_payload(form("--boundary\r\nContent-Disposition: form-data; name=\"response_format\"\r\n\r\ntext\r\n")).to_request()).await;
        assert_eq!(
            at::read_body(response).await,
            web::Bytes::from_static(b"transcribed")
        );
        let duplicate = form(
            "--boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nduplicate\r\n",
        );
        for (auth, content_type, body, expected) in [
            ("bad", "application/json", "{}".to_string(), 401),
            ("key", "application/json", "{}".to_string(), 400),
            (
                "key",
                "multipart/form-data; boundary=boundary",
                duplicate,
                400,
            ),
        ] {
            let response = at::call_service(
                &app,
                at::TestRequest::post()
                    .uri("/v1/audio/transcriptions")
                    .insert_header(("authorization", format!("Bearer {auth}")))
                    .insert_header(("content-type", content_type))
                    .set_payload(body)
                    .to_request(),
            )
            .await;
            assert_eq!(response.status().as_u16(), expected);
        }
        let response = at::call_service(
            &app,
            at::TestRequest::post()
                .uri("/v1/audio/transcriptions")
                .insert_header(("authorization", "Bearer key"))
                .insert_header(("content-type", "multipart/form-data; boundary=boundary"))
                .set_payload(form(
                    "--boundary\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\nfail\r\n",
                ))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), 503);
        assert_eq!(
            state
                .store
                .account_by_id("audio-account")
                .unwrap()
                .login_state,
            "login_expired"
        );
    }
    #[actix_web::test]
    async fn cancellation_stops_transcription_without_overwriting_terminal_state() {
        let state = state();
        let app = at::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let execution = at::call_service(
            &app,
            at::TestRequest::post()
                .uri("/v1/audio/transcriptions")
                .insert_header(("authorization", "Bearer key"))
                .insert_header(("content-type", "multipart/form-data; boundary=boundary"))
                .set_payload(form(
                    "--boundary\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\nslow\r\n",
                ))
                .to_request(),
        );
        let cancel = async {
            loop {
                if let Some(task) = state
                    .store
                    .list_tasks(None)
                    .into_iter()
                    .find(|task| task.status == "processing")
                {
                    state.store.cancel_task(&task.id);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        let (response, ()) = tokio::join!(execution, cancel);
        assert_eq!(response.status(), 409);
        assert!(state
            .store
            .list_tasks(None)
            .iter()
            .all(|task| task.status == "cancelled"));
    }
}
