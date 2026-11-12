use serde_json::Value;

pub const MAX_FRAME: usize = 65_536;
pub const PROTOCOL_VERSION: u32 = 1;
pub const APP_ID: &str = "red-green.mosaico";
pub fn encode(value: &Value) -> Result<Vec<u8>, String> {
    let json = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if json.len() > MAX_FRAME {
        return Err("Mosaico 消息超过 65536 bytes".into());
    }
    let mut frame = Vec::with_capacity(8 + json.len());
    frame.extend_from_slice(b"RGP1");
    frame.extend_from_slice(&(json.len() as u32).to_le_bytes());
    frame.extend_from_slice(&json);
    Ok(frame)
}
#[derive(Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
    expected: usize,
}
impl FrameDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Value>, String> {
        let mut frames = vec![];
        for byte in bytes {
            self.buffer.push(*byte);
            if self.buffer.len() == 8 {
                if &self.buffer[..4] != b"RGP1" {
                    return Err("不是 red-green 业务帧".into());
                }
                self.expected = u32::from_le_bytes(self.buffer[4..8].try_into().unwrap()) as usize;
                if self.expected == 0 || self.expected > MAX_FRAME {
                    return Err("业务帧长度无效".into());
                }
            }
            if self.buffer.len() >= 8 && self.buffer.len() == self.expected + 8 {
                frames.push(serde_json::from_slice(&self.buffer[8..]).map_err(|e| e.to_string())?);
                self.buffer.clear();
            }
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fragmented_and_coalesced_frames_preserve_unicode() {
        // 独立手工编码：{"x":1} 长 7 bytes。
        let first = b"RGP1\x07\x00\x00\x00{\"x\":1}";
        let mut d = FrameDecoder::default();
        assert!(d.push(&first[..5]).unwrap().is_empty());
        let mut rest = first[5..].to_vec();
        rest.extend(encode(&json!({"project":"工具箱"})).unwrap());
        assert_eq!(
            d.push(&rest).unwrap(),
            vec![json!({"x":1}), json!({"project":"工具箱"})]
        );
    }

    #[test]
    fn bad_magic_oversize_and_invalid_json_are_rejected() {
        assert!(FrameDecoder::default()
            .push(b"log!\x00\x00\x00\x00")
            .is_err());
        assert!(FrameDecoder::default()
            .push(b"RGP1\x01\x00\x01\x00")
            .is_err());
        assert!(FrameDecoder::default()
            .push(b"RGP1\x01\x00\x00\x00{")
            .is_err());
        assert!(encode(&json!({"x":"x".repeat(65536)})).is_err());
    }
}
