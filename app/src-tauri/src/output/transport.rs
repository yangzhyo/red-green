use super::protocol::{encode, FrameDecoder};
use serde_json::Value;
use serialport::SerialPort;
use std::io::{Read, Write};
use std::time::Duration;
pub struct Link {
    port: Box<dyn SerialPort>,
    decoder: FrameDecoder,
}
impl Link {
    pub fn new(port: Box<dyn SerialPort>) -> Self {
        Self {
            port,
            decoder: FrameDecoder::default(),
        }
    }
    pub fn open(path: &str) -> Result<Self, String> {
        let port = serialport::new(path, 115200)
            .timeout(Duration::from_millis(20))
            .dtr_on_open(false)
            .open()
            .map_err(|e| e.to_string())?;
        Ok(Self::new(port))
    }
    pub fn read(&mut self) -> Result<Vec<Value>, String> {
        let mut bytes = [0; 4096];
        match self.port.read(&mut bytes) {
            Ok(0) => Err("串口已关闭".into()),
            Ok(n) => self.decoder.push(&bytes[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(vec![])
            }
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn send(&mut self, v: &Value) -> Result<(), String> {
        let frame = encode(v)?;
        let mut written = 0;
        let started = super::runtime::now_ms();
        while written < frame.len() {
            if super::runtime::now_ms().saturating_sub(started) > 100 {
                return Err("串口写入超时".into());
            }
            match self.port.write(&frame[written..]) {
                Ok(0) => return Err("串口写入中断".into()),
                Ok(n) => written += n,
                Err(e) => return Err(e.to_string()),
            }
        }
        // 不调用可能无限等待设备的 tcdrain；过期 nonce 由设备拒绝。
        Ok(())
    }
}
pub fn discover() -> Vec<String> {
    if let Ok(path) = std::env::var("RED_GREEN_MOSAICO_PORT") {
        return vec![path];
    }
    let mut ports: Vec<_> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(u)
                if u.vid == 0x303a && u.product.as_deref() == Some("red-green Mosaico") =>
            {
                Some(p.port_name)
            }
            _ => None,
        })
        .collect();
    ports.sort();
    ports
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::time::Duration;
    #[test]
    fn native_pty_receives_and_sends_real_framed_messages() {
        let (mut peer, mut device) = serialport::TTYPort::pair().unwrap();
        peer.set_timeout(Duration::from_millis(100)).unwrap();
        device.set_timeout(Duration::from_millis(100)).unwrap();
        let mut link = Link::new(Box::new(device));
        peer.write_all(b"RGP1\x07\x00\x00\x00{\"x\":1}").unwrap();
        assert_eq!(link.read().unwrap(), vec![json!({"x":1})]);
        link.send(&json!({"x":2})).unwrap();
        let mut b = [0; 15];
        peer.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"RGP1\x07\x00\x00\x00{\"x\":2}");
    }
}
