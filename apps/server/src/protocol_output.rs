//! 通用 Responses 富媒体输出及 Chat 扩展映射，不包含平台规则。
use serde_json::{json, Value};
use std::collections::HashMap;

fn field(item: &Value) -> Option<&'static str> {
    match item["type"].as_str()? {
        "image_generation_call" | "video_generation_call" => Some("media"),
        "reasoning" => Some("reasoning"),
        _ => None,
    }
}

pub(crate) fn enrich_chat(message: &mut Value, response: &Value) {
    for name in ["media", "reasoning"] {
        let items: Vec<_> = response["output"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| field(item) == Some(name))
            .cloned()
            .collect();
        if !items.is_empty() {
            message[name] = json!(items);
        }
    }
    let summary: String = response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "reasoning")
        .flat_map(|item| item["summary"].as_array().into_iter().flatten())
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !summary.is_empty() {
        message["reasoning_content"] = json!(summary);
    }
}

#[derive(Default)]
pub(crate) struct RichStream(HashMap<String, Value>);
impl RichStream {
    pub(crate) fn item(&mut self, item: &Value) -> Option<Value> {
        let name = field(item)?;
        let id = item["id"].as_str()?;
        if self.0.get(id) == Some(item) {
            return None;
        }
        self.0.insert(id.into(), item.clone());
        let mut delta = json!({});
        delta[name] = json!([item]);
        Some(delta)
    }
    pub(crate) fn finish(&mut self, response: &Value) -> Vec<Value> {
        response["output"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| self.item(item))
            .collect()
    }
}

// 续接时把生成的媒体转换为已有输入协议，避免插件将生成回执当成非法消息。
// 带加密正文的 reasoning 仍交由原生 Responses 模型续接。
pub(crate) fn continuation(response: &Value) -> Vec<Value> {
    response["output"].as_array().into_iter().flatten().filter_map(|item| {
        let content = match item["type"].as_str() {
            Some("reasoning") if item["encrypted_content"].is_null() => return None,
            Some("image_generation_call") => {
                let data = item["result"].as_str()?;
                let format = item["output_format"].as_str().unwrap_or("png");
                if !["png","jpeg","webp"].contains(&format) { return None; }
                json!({"type":"input_image","image_url":format!("data:image/{format};base64,{data}")})
            }
            Some("video_generation_call") => json!({"type":"output_text","text":format!("生成的视频：{}", item["result_url"].as_str()?)}),
            _ => return Some(item.clone()),
        };
        Some(json!({"type":"message","role":"assistant","content":[content]}))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response() -> Value {
        json!({"id":"resp_test","object":"response","created_at":1,"status":"completed","model":"text-model","output":[
            {"id":"msg","type":"message","role":"assistant","content":[{"type":"output_text","text":"图片和视频已完成"}]},
            {"id":"rs","type":"reasoning","status":"completed","summary":[{"type":"summary_text","text":"先确认素材内容"}]},
            {"id":"ig","type":"image_generation_call","status":"completed","result":"aGVsbG8=","output_format":"png"},
            {"id":"vg","type":"video_generation_call","status":"completed","result_url":"https://example.com/video.mp4","output_format":"mp4"}
        ],"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3}})
    }
    #[test]
    fn chat_and_stream_preserve_media_and_reasoning_with_deduplicated_snapshots() {
        let response = response();
        let chat = crate::protocol_request::chat_response(&response, "text-model");
        let message = &chat["choices"][0]["message"];
        assert_eq!(message["content"], "图片和视频已完成");
        assert_eq!(message["media"].as_array().unwrap().len(), 2);
        assert_eq!(message["media"][0]["result"], "aGVsbG8=");
        assert_eq!(
            message["media"][1]["result_url"],
            "https://example.com/video.mp4"
        );
        assert_eq!(message["reasoning_content"], "先确认素材内容");
        let mut stream = crate::protocol_stream::Encoder::new(true, "text-model".into(), false);
        stream.encode(&json!({"type":"response.created","response":response}));
        let event = json!({"type":"response.output_item.done","item":response["output"][2]});
        assert!(stream.encode(&event).contains("aGVsbG8="));
        assert!(stream.encode(&event).is_empty());
        let last = stream.encode(&json!({"type":"response.completed","response":response}));
        assert!(!last.contains("aGVsbG8="));
        assert!(last.contains("https://example.com/video.mp4"));
        assert!(last.contains("先确认素材内容"));
        assert!(last.ends_with("data: [DONE]\n\n"));
    }
    #[test]
    fn continuation_keeps_media_references_and_encrypted_reasoning_without_status_items() {
        let mut response = response();
        response["output"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"reasoning","encrypted_content":"opaque"}));
        let items = continuation(&response);
        assert_eq!(items.len(), 4);
        assert_eq!(
            items[1]["content"][0]["image_url"],
            "data:image/png;base64,aGVsbG8="
        );
        assert!(items[2]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("video.mp4"));
        assert_eq!(items[3]["encrypted_content"], "opaque");
        let chat = crate::protocol_request::chat_response(&response, "text-model");
        let request = crate::protocol_request::chat_to_responses(
            &json!({"messages":[chat["choices"][0]["message"]]}),
        )
        .unwrap();
        assert!(request["input"]
            .to_string()
            .contains("data:image/png;base64"));
    }
    #[actix_web::test]
    async fn http_responses_store_and_retrieve_media_and_chat_stream_delivers_it() {
        use actix_web::{test, App};
        let state = crate::protocol_api_tests::state();
        let script = state
            .plugin_loader
            .get_plugin_dir("test")
            .unwrap()
            .join("scripts/text.js");
        let response = response();
        std::fs::write(script,format!("function main(ctx){{const response={response};response.id='resp_'+ctx.request_id;host.emit({{type:'response.created',response:{{...response,output:[]}}}});for(const item of response.output)host.emit({{type:'response.output_item.done',item}});return {{response}}}}" )).unwrap();
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(crate::configure_routes),
        )
        .await;
        let stored: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::post()
                .uri("/v1/responses")
                .insert_header(("authorization", "Bearer key-a"))
                .set_json(json!({"model":"text-model","input":"返回媒体"}))
                .to_request(),
        )
        .await;
        assert_eq!(stored["output"][2]["result"], "aGVsbG8=");
        let retrieved: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get()
                .uri(&format!("/v1/responses/{}", stored["id"].as_str().unwrap()))
                .insert_header(("authorization", "Bearer key-a"))
                .to_request(),
        )
        .await;
        assert_eq!(retrieved["output"], stored["output"]);
        let denied = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/v1/responses/{}", stored["id"].as_str().unwrap()))
                .insert_header(("authorization", "Bearer key-b"))
                .to_request(),
        )
        .await;
        assert_eq!(denied.status(), 404);
        let streamed = test::call_service(&app,test::TestRequest::post().uri("/v1/chat/completions").insert_header(("authorization","Bearer key-a")).set_json(json!({"model":"text-model","messages":[{"role":"user","content":"返回媒体"}],"stream":true})).to_request()).await;
        assert_eq!(streamed.status(), 200);
        let bytes = test::read_body(streamed).await;
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("aGVsbG8="));
        assert!(text.contains("https://example.com/video.mp4"));
        assert!(text.contains("先确认素材内容"));
    }
}
