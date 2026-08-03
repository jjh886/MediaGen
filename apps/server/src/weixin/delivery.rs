use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};

use crate::store_weixin::WeixinTaskDelivery;
use crate::ServerState;

const IDLE_DELAY: Duration = Duration::from_secs(3);
const FAILURE_DELAY: Duration = Duration::from_secs(2);
const MAX_WEIXIN_RESULT_IMAGES: usize = 4;

pub(crate) async fn run_task_delivery_worker(state: actix_web::web::Data<ServerState>) {
    loop {
        match state.store.next_weixin_task_delivery() {
            Some(delivery) => {
                if let Err(error) = queue_task_delivery(&state, &delivery).await {
                    eprintln!(
                        "[mediagen-server] 准备微信任务结果失败: task={} error={error}",
                        delivery.task_id
                    );
                    tokio::time::sleep(FAILURE_DELAY).await;
                }
            }
            None => wait_or_wake(&state).await,
        }
    }
}

pub(crate) async fn queue_completed_response(state: &ServerState, task_id: &str, response: &Value) {
    if let Err(error) = queue_completed_response_inner(state, task_id, response).await {
        eprintln!("[mediagen-server] 微信任务完成通知准备失败: task={task_id} error={error}");
    }
}

pub(crate) fn queue_terminal_status(
    state: &ServerState,
    task_id: &str,
    status: &str,
    error_message: Option<&str>,
) {
    if let Err(error) = queue_terminal_status_inner(state, task_id, status, error_message) {
        eprintln!("[mediagen-server] 微信任务状态通知准备失败: task={task_id} error={error}");
    }
}

async fn queue_completed_response_inner(
    state: &ServerState,
    task_id: &str,
    response: &Value,
) -> Result<(), String> {
    let Some(_) = state.store.pending_weixin_task_link(task_id) else {
        return Ok(());
    };
    if suppress_task_notice(state, task_id)? {
        return Ok(());
    }
    state.store.enqueue_weixin_outbound(
        &format!("task-{task_id}-complete"),
        "text",
        &json!({ "text": format!("图片生成完成\n任务：{}", short_id(task_id)) }).to_string(),
    );
    let images = match persist_response_images(state, task_id, response).await {
        Ok(value) => value,
        Err(error) => {
            state.store.enqueue_weixin_outbound(
                &format!("task-{task_id}-image-error"),
                "text",
                &json!({ "text": format!("图片已生成，但微信回传图片准备失败：{error}") })
                    .to_string(),
            );
            Vec::new()
        }
    };
    for (index, path) in images.iter().enumerate() {
        state.store.enqueue_weixin_outbound(
            &format!("task-{task_id}-image-{index}"),
            "image",
            &json!({ "path": path }).to_string(),
        );
    }
    state.store.finish_weixin_task_delivery(task_id, "queued");
    state.weixin.wake.notify_waiters();
    Ok(())
}

async fn queue_task_delivery(
    state: &ServerState,
    delivery: &WeixinTaskDelivery,
) -> Result<(), String> {
    if suppress_task_notice(state, &delivery.task_id)? {
        return Ok(());
    }
    match delivery.status.as_str() {
        "completed" => queue_completed_fallback(state, delivery),
        "failed" => queue_failed(state, &delivery.task_id, delivery.error_message.as_deref()),
        "cancelled" => queue_cancelled(state, &delivery.task_id),
        _ => return Ok(()),
    }
    state
        .store
        .finish_weixin_task_delivery(&delivery.task_id, "queued");
    state.weixin.wake.notify_waiters();
    Ok(())
}

fn queue_terminal_status_inner(
    state: &ServerState,
    task_id: &str,
    status: &str,
    error_message: Option<&str>,
) -> Result<(), String> {
    let Some(_) = state.store.pending_weixin_task_link(task_id) else {
        return Ok(());
    };
    if suppress_task_notice(state, task_id)? {
        return Ok(());
    }
    match status {
        "failed" => queue_failed(state, task_id, error_message),
        "cancelled" => queue_cancelled(state, task_id),
        _ => return Ok(()),
    }
    state.store.finish_weixin_task_delivery(task_id, "queued");
    state.weixin.wake.notify_waiters();
    Ok(())
}

fn suppress_task_notice(state: &ServerState, task_id: &str) -> Result<bool, String> {
    if state.store.get_weixin_preferences().notify_weixin_tasks {
        return Ok(false);
    }
    state
        .store
        .finish_weixin_task_delivery(task_id, "suppressed");
    Ok(true)
}

fn queue_completed_fallback(state: &ServerState, delivery: &WeixinTaskDelivery) {
    state.store.enqueue_weixin_outbound(
        &format!("task-{}-complete", delivery.task_id),
        "text",
        &json!({ "text": format!("图片生成完成\n任务：{}", short_id(&delivery.task_id)) })
            .to_string(),
    );
    for (index, url) in result_urls(delivery)
        .iter()
        .take(MAX_WEIXIN_RESULT_IMAGES)
        .enumerate()
    {
        state.store.enqueue_weixin_outbound(
            &format!("task-{}-image-{index}", delivery.task_id),
            "image",
            &json!({ "url": url }).to_string(),
        );
    }
}

fn queue_failed(state: &ServerState, task_id: &str, error_message: Option<&str>) {
    let reason = error_message
        .map(|value| value.chars().take(160).collect::<String>())
        .unwrap_or_else(|| "生成失败".to_string());
    state.store.enqueue_weixin_outbound(
        &format!("task-{task_id}-failed"),
        "text",
        &json!({
            "text": format!("图片生成失败\n任务：{}\n原因：{}", short_id(task_id), reason)
        })
        .to_string(),
    );
}

fn queue_cancelled(state: &ServerState, task_id: &str) {
    state.store.enqueue_weixin_outbound(
        &format!("task-{task_id}-cancelled"),
        "text",
        &json!({ "text": format!("任务已取消\n任务：{}", short_id(task_id)) }).to_string(),
    );
}

async fn persist_response_images(
    state: &ServerState,
    task_id: &str,
    response: &Value,
) -> Result<Vec<String>, String> {
    let output_dir = state.file_root.join("weixin-outbound");
    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|error| format!("创建微信图片目录失败: {error}"))?;
    let mut paths = Vec::new();
    for (index, b64) in response_b64_images(response)
        .iter()
        .take(MAX_WEIXIN_RESULT_IMAGES)
        .enumerate()
    {
        let bytes = STANDARD
            .decode(b64)
            .map_err(|_| "生成图片 base64 无效".to_string())?;
        let extension = image_extension(&bytes)?;
        let path = output_dir.join(format!("{task_id}-{index}.{extension}"));
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|error| format!("写入微信图片失败: {error}"))?;
        paths.push(path.to_string_lossy().to_string());
    }
    Ok(paths)
}

fn response_b64_images(response: &Value) -> Vec<String> {
    response
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("b64_json").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn result_urls(delivery: &WeixinTaskDelivery) -> Vec<String> {
    let result_data = delivery
        .result_data
        .as_deref()
        .and_then(|value| serde_json::from_str::<Value>(value).ok());
    let mut urls = result_data
        .as_ref()
        .and_then(|value| value["resultUrls"].as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if urls.is_empty() {
        if let Some(url) = delivery
            .result_url
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            urls.push(url.clone());
        }
    }
    urls
}

fn image_extension(bytes: &[u8]) -> Result<&'static str, String> {
    match image::guess_format(bytes).map_err(|_| "生成结果不是图片".to_string())? {
        image::ImageFormat::Png => Ok("png"),
        image::ImageFormat::Jpeg => Ok("jpg"),
        image::ImageFormat::WebP => Ok("webp"),
        _ => Err("微信回传图片只支持 PNG、JPEG 或 WebP".to_string()),
    }
}

async fn wait_or_wake(state: &actix_web::web::Data<ServerState>) {
    tokio::select! {
        _ = state.weixin.wake.notified() => {}
        _ = tokio::time::sleep(IDLE_DELAY) => {}
    }
}

fn short_id(value: &str) -> &str {
    value.get(..8).unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_image_response_items() {
        let response = json!({ "data": [{ "b64_json": "abc" }, { "url": "https://x" }] });
        assert_eq!(response_b64_images(&response), vec!["abc".to_string()]);
    }
}
