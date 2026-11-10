use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{hash_map::DefaultHasher, HashSet};
use std::hash::{Hash, Hasher};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pet {
    pub sid: String,
    pub slot: u32,
    pub model: Value,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OutputSnapshot {
    pub pets: Vec<Pet>,
    pub desktop_usage: Vec<Value>,
    pub device_usage: Vec<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Call {
    pub sid: String,
    pub name: String,
}
impl OutputSnapshot {
    pub fn wire(&self, generation: &str, version: u64) -> Result<Value, String> {
        if self.pets.len() > 32 || self.device_usage.len() > 32 {
            return Err("设备快照超过容量，保留桌面输出".into());
        }
        let mut ordered: Vec<_> = self.pets.iter().collect();
        ordered.sort_by_key(|p| p.slot);
        let mut ids = HashSet::new();
        let mut sessions = vec![];
        for p in ordered {
            let source = p.model["source"].as_str().unwrap_or("claude");
            if !["claude", "codex"].contains(&source) {
                return Err("未知会话来源".into());
            }
            let project = bounded(&p.model, "project", 192)?;
            let skin = bounded(&p.model, "skin", 16)?;
            if !["robot", "crab", "cat"].contains(&skin.as_str()) {
                return Err("设备不支持该皮肤".into());
            }
            let state = p.model["state"].as_str().unwrap_or("idle");
            let state = if [
                "idle",
                "running",
                "awaiting",
                "your_turn",
                "completed",
                "aborted",
            ]
            .contains(&state)
            {
                state
            } else {
                "idle"
            };
            let id = opaque_id(&p.sid);
            if !ids.insert(id.clone()) {
                return Err("重复会话身份".into());
            }
            let duplicate = self
                .pets
                .iter()
                .filter(|q| {
                    q.model["project"] == p.model["project"]
                        && q.model["source"] == p.model["source"]
                })
                .count()
                > 1;
            let name = if duplicate {
                format!("{project} #{}", p.slot + 1)
            } else {
                project
            };
            sessions.push(json!({"id":id,"slot":p.slot,"name":name,"source":source,"skin":skin,"state":state}));
        }
        let mut usage = vec![];
        for g in &self.device_usage {
            let source = bounded(g, "source", 16)?;
            if !["claude", "codex"].contains(&source.as_str()) {
                return Err("未知用量来源".into());
            }
            let windows = g["windows"].as_array().ok_or("缺少用量窗口")?;
            if windows.len() > 32 {
                return Err("用量窗口数量超限".into());
            }
            let mut projected = vec![];
            for w in windows {
                let percentage = w["used_percentage"]
                    .as_f64()
                    .filter(|p| (0.0..=100.0).contains(p));
                projected.push(json!({"used_percentage":percentage,"duration_label":bounded(w,"duration_label",32)?,"reset_label":bounded(w,"reset_label",48)?,"status":if percentage.is_some(){"known"}else if w["status"]=="expired"{"expired"}else{"unknown"}}));
            }
            usage.push(json!({"id":bounded(g,"id",192)?,"source":source,"label":bounded(g,"label",128)?,"windows":projected}));
        }
        let out = json!({"type":"snapshot","generation":generation,"version":version,"sessions":sessions,"usage":usage});
        super::protocol::encode(&out)?;
        Ok(out)
    }
}
pub fn opaque_id(sid: &str) -> String {
    let mut h = DefaultHasher::new();
    sid.hash(&mut h);
    format!("s{:016x}", h.finish())
}
fn bounded(v: &Value, key: &str, max: usize) -> Result<String, String> {
    let text = v[key].as_str().unwrap_or("");
    if text.len() > max || text.chars().any(char::is_control) {
        return Err(format!("设备字段 {key} 超长或含控制字符"));
    }
    Ok(text.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn snapshot() -> OutputSnapshot {
        OutputSnapshot {
            pets: vec![Pet {
                sid: "real-session-id".into(),
                slot: 0,
                model: json!({"source":"codex","project":"工具箱","skin":"cat","state":"awaiting","tty":"/dev/private","cwd":"/private/path","detail":"secret conversation"}),
            }],
            ..Default::default()
        }
    }
    #[test]
    fn wire_contains_display_fields_without_private_model_data() {
        let s = snapshot();
        let wire = s.wire("generation1", 1).unwrap();
        assert_eq!(wire["sessions"][0]["name"], "工具箱");
        assert_eq!(wire["sessions"][0]["state"], "awaiting");
        let text = wire.to_string();
        for forbidden in [
            "real-session-id",
            "/dev/private",
            "/private/path",
            "secret conversation",
            "tty",
            "cwd",
            "detail",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
    #[test]
    fn duplicate_project_names_have_distinct_stable_identities_and_numbers() {
        let mut s = snapshot();
        let mut second = s.pets[0].clone();
        second.sid = "other-session".into();
        second.slot = 1;
        s.pets.push(second);
        let w = s.wire("g", 1).unwrap();
        assert_ne!(w["sessions"][0]["id"], w["sessions"][1]["id"]);
        assert_eq!(w["sessions"][0]["name"], "工具箱 #1");
        assert_eq!(w["sessions"][1]["name"], "工具箱 #2");
    }
    #[test]
    fn invalid_skin_or_oversize_payload_cannot_take_over() {
        let mut s = snapshot();
        s.pets[0].model["skin"] = json!("unknown");
        assert!(s.wire("g", 1).is_err());
        let mut s = snapshot();
        s.pets[0].model["project"] = json!("x".repeat(65536));
        assert!(s.wire("g", 1).is_err());
    }
}
