use serde_json::{json, Value};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outdated_client_errors_keep_actionable_codes_in_both_stream_formats() {
        let (status, code, message) =
            crate::protocol_api::public_failure("upstream_client_outdated: private-upstream-token");
        assert_eq!(status, 503);
        assert_eq!(code, "upstream_client_outdated");
        assert!(message.contains("更新对应插件"));
        for chat in [false, true] {
            let mut encoder = Encoder::new(chat, "test-model".into(), false);
            let body = encoder.encode(&json!({
                "type": "error", "status": status,
                "error": {"code": code, "message": message}
            }));
            assert!(body.contains(code));
            assert!(!body.contains("private-upstream-token"));
            assert!(encoder.terminal);
        }
    }
}

pub(crate) struct Encoder {
    chat: bool,
    model: String,
    id: String,
    created: i64,
    tools: HashMap<String, usize>,
    include_usage: bool,
    rich: crate::protocol_output::RichStream,
    pub terminal: bool,
}

impl Encoder {
    pub fn new(chat: bool, model: String, include_usage: bool) -> Self {
        Self {
            chat,
            model,
            id: String::new(),
            created: chrono::Utc::now().timestamp(),
            tools: HashMap::new(),
            include_usage,
            rich: Default::default(),
            terminal: false,
        }
    }
    pub fn encode(&mut self, event: &Value) -> String {
        let mut event = event.clone();
        if let Some(object) = event.as_object_mut() {
            object.remove("status");
        }
        let kind = event["type"].as_str().unwrap_or("error");
        if !self.chat {
            self.terminal |= matches!(
                kind,
                "response.completed" | "response.failed" | "response.incomplete" | "error"
            );
            return format!("event: {kind}\ndata: {event}\n\n");
        }
        match kind {
            "response.created" => {
                self.id = event["response"]["id"]
                    .as_str()
                    .unwrap_or("")
                    .replace("resp_", "chatcmpl-");
                self.created = event["response"]["created_at"]
                    .as_i64()
                    .unwrap_or(self.created);
                self.chunk(json!({"role":"assistant","content":""}), Value::Null)
            }
            "response.output_text.delta" => {
                self.chunk(json!({"content":event["delta"]}), Value::Null)
            }
            "response.refusal.delta" => self.chunk(json!({"refusal":event["delta"]}), Value::Null),
            "response.output_item.added" if event["item"]["type"] == "function_call" => {
                let index = self.tools.len();
                self.tools
                    .insert(event["item"]["id"].as_str().unwrap_or("").into(), index);
                self.chunk(json!({"tool_calls":[{"index":index,"id":event["item"]["call_id"],"type":"function","function":{"name":event["item"]["name"],"arguments":""}}]}), Value::Null)
            }
            "response.reasoning_summary_text.delta" => {
                self.chunk(json!({"reasoning_content":event["delta"]}), Value::Null)
            }
            "response.output_item.added" | "response.output_item.done" => self
                .rich
                .item(&event["item"])
                .map(|delta| self.chunk(delta, Value::Null))
                .unwrap_or_default(),
            "response.function_call_arguments.delta" => {
                let index = self.tools.get(event["item_id"].as_str().unwrap_or(""));
                index.map(|index| self.chunk(json!({"tool_calls":[{"index":index,"function":{"arguments":event["delta"]}}]}),Value::Null)).unwrap_or_default()
            }
            "response.completed" | "response.incomplete" => {
                self.terminal = true;
                let mut result = self
                    .rich
                    .finish(&event["response"])
                    .into_iter()
                    .map(|delta| self.chunk(delta, Value::Null))
                    .collect::<String>();
                result.push_str(&self.chunk_with_metadata(
                    json!({}),
                    json!(crate::protocol_request::finish_reason(
                        &event["response"],
                        !self.tools.is_empty()
                    )),
                    event["response"]["metadata"].as_object(),
                ));
                if self.include_usage {
                    let usage = json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[],"usage":crate::protocol_request::chat_usage(&event["response"]["usage"])});
                    result.push_str(&format!("data: {usage}\n\n"));
                }
                result.push_str("data: [DONE]\n\n");
                result
            }
            "response.failed" | "error" => {
                self.terminal = true;
                let error = event.get("error").or_else(|| event["response"].get("error")).cloned().unwrap_or_else(|| json!({"message":"上游事件流失败","type":"server_error","code":"upstream_error","param":null}));
                format!("data: {}\n\ndata: [DONE]\n\n", json!({"error":error}))
            }
            _ => String::new(),
        }
    }
    fn chunk(&self, delta: Value, reason: Value) -> String {
        self.chunk_with_metadata(delta, reason, None)
    }
    fn chunk_with_metadata(
        &self,
        delta: Value,
        reason: Value,
        metadata: Option<&serde_json::Map<String, Value>>,
    ) -> String {
        let mut chunk = json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":reason,"logprobs":null}]});
        if self.include_usage {
            chunk["usage"] = Value::Null;
        }
        if let Some(metadata) = metadata.filter(|metadata| !metadata.is_empty()) {
            chunk["metadata"] = json!(metadata);
        }
        format!("data: {chunk}\n\n")
    }
}
