use serde_json::{json, Value};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

/// 只读客户端；不会 resume/subscribe，也不会响应用户的审批请求。
pub struct Rpc {
    ws: WebSocket<UnixStream>,
    next_id: u64,
    pub account_changed: bool,
}

impl Rpc {
    pub fn connect(path: &Path) -> Result<Self, String> {
        Self::from_stream(UnixStream::connect(path).map_err(|e| e.to_string())?)
    }

    pub(super) fn from_stream(stream: UnixStream) -> Result<Self, String> {
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| e.to_string())?;
        let (ws, _) = tungstenite::client("ws://localhost/", stream).map_err(|e| e.to_string())?;
        let mut rpc = Self {
            ws,
            next_id: 0,
            account_changed: false,
        };
        rpc.call(
            "initialize",
            json!({"clientInfo":{"name":"red_green","version":"0.1.0"},
            "capabilities":{"experimentalApi":true}}),
        )?;
        rpc.ws
            .send(Message::text(json!({"method":"initialized"}).to_string()))
            .map_err(|e| e.to_string())?;
        Ok(rpc)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.ws
            .send(Message::text(
                json!({"id":id,"method":method,"params":params}).to_string(),
            ))
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("Codex 请求超时")?;
            self.ws
                .get_mut()
                .set_read_timeout(Some(remaining))
                .map_err(|e| e.to_string())?;
            let msg = self.ws.read().map_err(|e| e.to_string())?;
            if msg.is_close() {
                return Err("Codex 连接已关闭".into());
            }
            if msg.is_ping() {
                self.ws.flush().map_err(|e| e.to_string())?;
                continue;
            }
            if !msg.is_text() {
                continue;
            }
            let v: Value = serde_json::from_str(msg.to_text().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if v["method"] == "account/updated" {
                self.account_changed = true;
            }
            if v["id"].as_u64() != Some(id) {
                continue;
            }
            if let Some(error) = v.get("error") {
                return Err(error.to_string());
            }
            return v
                .get("result")
                .cloned()
                .ok_or_else(|| "Codex 响应缺少 result".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::net::UnixStream;

    #[test]
    fn reads_own_response_after_notifications_and_surfaces_rpc_error() {
        let (a, b) = UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            let mut ws = tungstenite::accept(b).unwrap();
            let init: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(init["method"], "initialize");
            ws.send(tungstenite::Message::text(
                json!({"id":init["id"],"result":{}}).to_string(),
            ))
            .unwrap();
            let ready: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(ready["method"], "initialized");
            let request: Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(request["method"], "thread/read");
            assert_eq!(request["params"]["includeTurns"], false);
            for message in [
                json!({"method":"thread/status/changed","params":{}}),
                json!({"id":999,"result":null}),
                json!({"id":request["id"],"result":{"thread":{"id":"the-thread"}}}),
            ] {
                ws.send(tungstenite::Message::text(message.to_string()))
                    .unwrap();
            }
            let request: Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            ws.send(tungstenite::Message::text(
                json!({"id":request["id"],"error":{"code":-32601,"message":"unsupported"}})
                    .to_string(),
            ))
            .unwrap();
        });
        let mut rpc = Rpc::from_stream(a).unwrap();
        assert_eq!(
            rpc.call(
                "thread/read",
                json!({"threadId":"the-thread","includeTurns":false})
            )
            .unwrap()["thread"]["id"],
            "the-thread"
        );
        assert!(rpc
            .call("account/rateLimits/read", json!({}))
            .unwrap_err()
            .contains("unsupported"));
        server.join().unwrap();
    }
}
