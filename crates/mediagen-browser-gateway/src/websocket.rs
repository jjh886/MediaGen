use std::io::{self, Cursor};

use bytes::{Bytes, BytesMut};
use reqwest::{RequestBuilder, Response};
use tokio::io::{join, split, Join, ReadHalf, WriteHalf};
use tokio_tungstenite::{
    tungstenite::{
        handshake::derive_accept_key,
        protocol::{
            frame::{coding::OpCode, FrameHeader},
            Role, WebSocketConfig,
        },
    },
    WebSocketStream,
};
use tokio_util::{
    codec::{Decoder, FramedRead},
    io::StreamReader,
};

/// 先收齐实际字节，再交给 WebSocket 库，防止仅凭对端声明的帧长分配内存。
pub fn frame_length(bytes: &[u8], masked: bool) -> io::Result<Option<usize>> {
    let mut cursor = Cursor::new(bytes);
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "WebSocket 帧无效");
    let Some((header, size)) = FrameHeader::parse(&mut cursor).map_err(|_| invalid())? else {
        return Ok(None);
    };
    if header.mask.is_some() != masked
        || header.rsv1
        || header.rsv2
        || header.rsv3
        || (matches!(header.opcode, OpCode::Control(_)) && (!header.is_final || size > 125))
    {
        return Err(invalid());
    }
    size.checked_add(cursor.position())
        .filter(|&n| n <= isize::MAX as u64)
        .map(|n| Some(n as usize))
        .ok_or_else(invalid)
}

#[derive(Debug)]
pub struct ServerFrames;
impl Decoder for ServerFrames {
    type Item = Bytes;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<Bytes>> {
        Ok(frame_length(buf, false)?
            .filter(|&len| len <= buf.len())
            .map(|len| buf.split_to(len).freeze()))
    }
}

type Upgraded = reqwest::Upgraded;
pub type Socket = WebSocketStream<
    Join<StreamReader<FramedRead<ReadHalf<Upgraded>, ServerFrames>, Bytes>, WriteHalf<Upgraded>>,
>;

pub fn upgrade_request(request: RequestBuilder, key: &str) -> RequestBuilder {
    request
        .version(reqwest::Version::HTTP_11)
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", key)
}

pub async fn finish_upgrade(response: Response, key: &str) -> Result<Socket, String> {
    let headers = response.headers();
    let has_token = |name: &str, token: &str| {
        headers
            .get_all(name)
            .iter()
            .filter_map(|header| header.to_str().ok())
            .flat_map(|text| text.split(','))
            .any(|value| value.trim().eq_ignore_ascii_case(token))
    };
    if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS
        || !has_token("connection", "upgrade")
        || !has_token("upgrade", "websocket")
        || headers
            .get("sec-websocket-accept")
            .and_then(|value| value.to_str().ok())
            != Some(derive_accept_key(key.as_bytes()).as_str())
    {
        return Err(format!(
            "上游拒绝 WebSocket 握手（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let stream = response.upgrade().await.map_err(|_| "WebSocket 升级失败")?;
    let (read, write) = split(stream);
    let io = join(
        StreamReader::new(FramedRead::new(read, ServerFrames)),
        write,
    );
    let config = WebSocketConfig::default()
        .max_message_size(None)
        .max_frame_size(None);
    Ok(WebSocketStream::from_raw_socket(io, Role::Client, Some(config)).await)
}
