/// 把事件名和 JSON 数据格式化成标准 SSE 字节。
/// 拆成小工具是为了让任务队列和账号队列都能复用同一种输出格式。
pub(crate) fn sse_bytes(event: &str, data: &serde_json::Value) -> bytes::Bytes {
    bytes::Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
}
