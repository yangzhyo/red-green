use super::{model, rpc::Rpc};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Default)]
pub struct Snapshot {
    pub sessions: Vec<Value>,
    pub usage: Value,
}

#[derive(Default)]
pub struct Observer {
    projects: BTreeMap<String, String>,
    last_usage_read: Option<Instant>,
    usage: Value,
}

impl Observer {
    pub fn poll(
        &mut self,
        rpc: &mut Rpc,
        bindings: &BTreeMap<String, String>,
    ) -> Result<Snapshot, String> {
        let mut loaded = Vec::new();
        let mut cursor = Value::Null;
        loop {
            let page = rpc.call("thread/loaded/list", json!({"cursor":cursor}))?;
            loaded.extend(
                page["data"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_owned)),
            );
            cursor = page["nextCursor"].clone();
            if cursor.is_null() {
                break;
            }
        }
        let bindings: BTreeMap<String, String> = bindings
            .iter()
            .filter_map(|(hint, tty)| model::resolve_id(hint, &loaded).map(|id| (id, tty.clone())))
            .collect();
        let mut sessions = Vec::new();
        self.projects.retain(|id, _| bindings.contains_key(id));
        for (id, tty) in &bindings {
            let thread = rpc.call("thread/read", json!({"threadId":id,"includeTurns":false}))?
                ["thread"]
                .clone();
            if thread["status"]["type"] == "notLoaded" {
                continue;
            }
            let turns = match rpc.call(
                "thread/turns/list",
                json!({"threadId":id,"limit":1,"itemsView":"summary"}),
            ) {
                Ok(turns) => turns,
                // 新 CLI 在第一条用户消息前有 thread ID，但尚无持久化回合。
                Err(error)
                    if serde_json::from_str::<Value>(&error).is_ok_and(|e| {
                        e["code"] == -32600
                            && e["message"]
                                .as_str()
                                .is_some_and(|s| s.contains("not materialized yet"))
                    }) =>
                {
                    json!({"data":[]})
                }
                Err(error) => return Err(error),
            };
            let project = self
                .projects
                .entry(id.clone())
                .or_insert_with(|| project_name(thread["cwd"].as_str().unwrap_or("")));
            let session = model::session(
                &thread,
                turns["data"].as_array().and_then(|a| a.first()),
                tty,
                project,
            );
            if !session.is_null() {
                sessions.push(session);
            }
        }
        if rpc.account_changed {
            self.invalidate_account(rpc);
        }
        if !sessions.is_empty()
            && self
                .last_usage_read
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(60))
        {
            self.last_usage_read = Some(Instant::now());
            if let Ok(raw) = rpc.call("account/rateLimits/read", json!({})) {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.usage = model::usage(&raw, now);
            }
        }
        // 通知也可能在额度请求等待期间到达；不发布跨账号的旧响应。
        if rpc.account_changed {
            self.invalidate_account(rpc);
        }
        Ok(Snapshot {
            sessions,
            usage: self.usage.clone(),
        })
    }

    fn invalidate_account(&mut self, rpc: &mut Rpc) {
        self.usage = Value::Null;
        self.last_usage_read = None;
        rpc.account_changed = false;
    }
}

fn project_name(cwd: &str) -> String {
    let root = Command::new("git")
        .args(["-C", cwd, "rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    std::path::Path::new(root.as_deref().unwrap_or(cwd))
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("?")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_change_during_quota_read_never_publishes_old_account() {
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            let mut ws = tungstenite::accept(server).unwrap();
            let mut quotas = 0;
            while let Ok(message) = ws.read() {
                let r: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                let result = match r["method"].as_str().unwrap() {
                    "initialized" => continue,
                    "initialize" => json!({}),
                    "thread/loaded/list" => {
                        json!({"data":["019a1234-1234-7123-8123-123456789abc"]})
                    }
                    "thread/read" => {
                        json!({"thread":{"id":r["params"]["threadId"],"cwd":"/tmp/project","status":{"type":"idle"}}})
                    }
                    "thread/turns/list" => json!({"data":[]}),
                    "account/rateLimits/read" => {
                        quotas += 1;
                        if quotas == 1 {
                            ws.send(tungstenite::Message::text(
                                json!({"method":"account/updated"}).to_string(),
                            ))
                            .unwrap();
                        }
                        json!({"accountId":format!("account-{quotas}"),"rateLimits":{"primary":{"usedPercent":23}}})
                    }
                    other => panic!("unexpected: {other}"),
                };
                ws.send(tungstenite::Message::text(
                    json!({"id":r["id"],"result":result}).to_string(),
                ))
                .unwrap();
            }
        });
        let mut rpc = Rpc::from_stream(client).unwrap();
        let mut observer = Observer::default();
        let bindings = BTreeMap::from([(
            "019a1234-1234-7123-8123-123456789abc".into(),
            "/dev/ttys001".into(),
        )]);
        assert!(observer.poll(&mut rpc, &bindings).unwrap().usage.is_null());
        assert_eq!(
            observer.poll(&mut rpc, &bindings).unwrap().usage["groups"][0]["account_id"],
            "account-2"
        );
        drop(rpc);
        worker.join().unwrap();
    }

    #[test]
    fn two_clients_share_quota_and_account_change_invalidates_cache() {
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            let mut ws = tungstenite::accept(server).unwrap();
            let mut quotas = 0;
            let mut reads = 0;
            while let Ok(message) = ws.read() {
                let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                let method = request["method"].as_str().unwrap();
                if method == "initialized" {
                    continue;
                }
                let result = match method {
                    "initialize" => json!({}),
                    "thread/loaded/list" => {
                        json!({"data":["019a1234-1234-7123-8123-123456789abc","019a1234-1234-7123-8123-123456789abd"]})
                    }
                    "thread/read" => {
                        reads += 1;
                        if reads == 5 {
                            ws.send(tungstenite::Message::text(
                                json!({"method":"account/updated"}).to_string(),
                            ))
                            .unwrap();
                        }
                        json!({"thread":{"id":request["params"]["threadId"],"cwd":"/tmp/project","status":{"type":if reads == 2 {"active"} else {"idle"},"activeFlags":[]}}})
                    }
                    "thread/turns/list" if reads == 1 => {
                        ws.send(tungstenite::Message::text(json!({"id":request["id"],"error":{"code":-32600,"message":"thread is not materialized yet; thread/turns/list is unavailable before first user message"}}).to_string())).unwrap();
                        continue;
                    }
                    "thread/turns/list" => {
                        json!({"data":[{"id":"turn","status":"completed","items":[{"type":"agentMessage","text":"完成。"}],"completedAt":123}]})
                    }
                    "account/rateLimits/read" => {
                        quotas += 1;
                        json!({"accountId":format!("account-{quotas}"),"rateLimits":{"primary":{"usedPercent":23,"windowDurationMins":300,"resetsAt":9999999999u64}}})
                    }
                    other => panic!("不应调用写接口：{other}"),
                };
                ws.send(tungstenite::Message::text(
                    json!({"id":request["id"],"result":result}).to_string(),
                ))
                .unwrap();
            }
            quotas
        });
        let mut rpc = Rpc::from_stream(client).unwrap();
        let mut observer = Observer::default();
        let mut bindings = BTreeMap::from([
            (
                "019a1234-1234-7123-8123-123456789abc".into(),
                "/dev/ttys001".into(),
            ),
            (
                "019a1234-1234-7123-8123-123456789abd".into(),
                "/dev/ttys002".into(),
            ),
        ]);
        let first = observer.poll(&mut rpc, &bindings).unwrap();
        assert_eq!(first.sessions.len(), 2);
        assert_eq!(first.sessions[0]["state"], "idle");
        assert_eq!(first.sessions[1]["state"], "running");
        let second = observer.poll(&mut rpc, &bindings).unwrap();
        assert_eq!(second.sessions[0]["state"], "completed");
        assert_eq!(first.usage, second.usage);
        bindings.remove("019a1234-1234-7123-8123-123456789abd");
        let third = observer.poll(&mut rpc, &bindings).unwrap();
        assert_eq!(third.sessions.len(), 1);
        assert_eq!(third.usage["groups"][0]["account_id"], "account-2");
        drop(rpc);
        assert_eq!(worker.join().unwrap(), 2);
    }
}
