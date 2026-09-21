//! 增量分帧只扫描新字节；大事件分片不会反复扫描或移动整个缓冲区。
pub(crate) struct Decoder {
    pending: Vec<u8>,
    line_start: usize,
    after_cr: bool,
    limit: usize,
    at_start: bool,
    bom_prefix: usize,
}
impl Decoder {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            pending: Vec::new(),
            line_start: 0,
            after_cr: false,
            limit,
            at_start: true,
            bom_prefix: 0,
        }
    }
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut blocks = Vec::new();
        for &byte in bytes {
            if self.at_start {
                const BOM: &[u8] = b"\xef\xbb\xbf";
                if byte == BOM[self.bom_prefix] {
                    self.bom_prefix += 1;
                    if self.bom_prefix == BOM.len() {
                        self.at_start = false;
                        self.bom_prefix = 0;
                    }
                    continue;
                }
                self.at_start = false;
                self.pending.extend_from_slice(&BOM[..self.bom_prefix]);
                self.bom_prefix = 0;
            }
            if self.after_cr {
                self.after_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            if matches!(byte, b'\r' | b'\n') {
                self.after_cr = byte == b'\r';
                if self.pending.len() == self.line_start {
                    if !self.pending.is_empty() {
                        blocks.push(std::mem::take(&mut self.pending));
                    }
                    self.line_start = 0;
                } else {
                    if self.pending.len() >= self.limit {
                        return Err("单个事件流消息超过大小限制".into());
                    }
                    self.pending.push(b'\n');
                    self.line_start = self.pending.len();
                }
            } else {
                if self.pending.len() >= self.limit {
                    return Err("单个事件流消息超过大小限制".into());
                }
                self.pending.push(byte);
            }
        }
        Ok(blocks)
    }
    pub(crate) fn finish(mut self) -> Vec<u8> {
        if self.at_start {
            self.pending
                .extend_from_slice(&b"\xef\xbb\xbf"[..self.bom_prefix]);
        }
        self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_byte_split_preserves_unicode_cr_lf_and_truncated_tail() {
        let input = "\u{feff}data: 一\r\n\r\ndata: 二\r\rid: 2\ndata: 三\n\npartial";
        for width in 1..=input.len() {
            let mut decoder = Decoder::new(128);
            let mut blocks = Vec::new();
            for bytes in input.as_bytes().chunks(width) {
                blocks.extend(decoder.push(bytes).unwrap());
            }
            assert_eq!(
                blocks,
                vec![
                    "data: 一\n".as_bytes(),
                    "data: 二\n".as_bytes(),
                    "id: 2\ndata: 三\n".as_bytes()
                ]
            );
            assert_eq!(decoder.finish(), b"partial");
        }
    }
    #[test]
    fn budget_is_per_event_and_not_per_connection() {
        let mut decoder = Decoder::new(10);
        for _ in 0..100 {
            assert_eq!(decoder.push(b"data: 1\n\n").unwrap().len(), 1);
        }
        assert!(decoder.push(b"12345678901").is_err());
    }
}
