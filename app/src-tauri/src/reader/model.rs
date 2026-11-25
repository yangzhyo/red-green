use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const PAGE: usize = 24;

#[derive(Clone, Serialize)]
pub struct Listing {
    pub key: String,
    pub project: String,
    pub source: String,
}

#[derive(Clone, Serialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub order: usize,
    #[serde(skip)]
    version: u64,
}

pub struct Conversation {
    listing: Listing,
    messages: Vec<Message>,
    version: u64,
    generation: u64,
}

impl Conversation {
    pub fn new(key: &str, project: &str, source: &str) -> Self {
        Self {
            listing: Listing {
                key: key.into(),
                project: project.into(),
                source: source.into(),
            },
            messages: Vec::new(),
            generation: rand::random(),
            version: 0,
        }
    }

    pub fn put(&mut self, id: &str, role: &str, text: String) {
        if let Some(message) = self.messages.iter_mut().find(|m| m.id == id) {
            if message.text != text {
                self.version += 1;
                message.text = text;
                message.version = self.version;
            }
            return;
        }
        self.version += 1;
        self.messages.push(Message {
            id: id.into(),
            role: role.into(),
            text,
            order: self.messages.len(),
            version: self.version,
        });
    }

    fn cursor(&self, value: u64) -> String {
        format!("{:x}-{value}", self.generation)
    }

    fn position(&self, cursor: &str) -> Option<u64> {
        let (generation, value) = cursor.split_once('-')?;
        (u64::from_str_radix(generation, 16).ok()? == self.generation)
            .then(|| value.parse().ok())
            .flatten()
    }

    fn page(&self, query: &str) -> Value {
        let since = query
            .strip_prefix("since=")
            .and_then(|s| self.position(s))
            .filter(|&v| v <= self.version);
        if let Some(version) = since {
            let messages: Vec<_> = self
                .messages
                .iter()
                .filter(|m| m.version > version)
                .collect();
            return json!({"messages": messages, "since":self.cursor(self.version)});
        }
        let before = query
            .strip_prefix("before=")
            .and_then(|s| self.position(s))
            .and_then(|v| usize::try_from(v).ok())
            .filter(|&v| v <= self.messages.len());
        let end = before.unwrap_or(self.messages.len());
        let start = end.saturating_sub(PAGE);
        json!({"messages":&self.messages[start..end], "before": (start > 0).then(||self.cursor(start as u64)),
            "since":self.cursor(self.version), "reset": !query.is_empty() && before.is_none()})
    }
}

#[derive(Default)]
pub struct Library {
    pub sessions: BTreeMap<String, Conversation>,
}

impl Library {
    pub fn add(&mut self, conversation: Conversation) {
        self.sessions
            .insert(conversation.listing.key.clone(), conversation);
    }
    pub fn list(&self) -> Value {
        json!(self
            .sessions
            .values()
            .map(|c| &c.listing)
            .collect::<Vec<_>>())
    }
    pub fn messages(&self, key: &str, query: &str) -> Option<Value> {
        self.sessions.get(key).map(|c| c.page(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_does_not_consume_live_updates() {
        let mut thread = Conversation::new("demo", "示例", "样本");
        for n in 0..30 {
            thread.put(&n.to_string(), "user", format!("消息{n}"));
        }
        let first = thread.page("");
        assert_eq!(first["messages"][0]["id"], "6");
        thread.put("29", "user", "更长的回复".into());
        let older = thread.page(&format!("before={}", first["before"].as_str().unwrap()));
        assert_eq!(older["messages"].as_array().unwrap().len(), 6);
        let update = thread.page(&format!("since={}", first["since"].as_str().unwrap()));
        assert_eq!(update["messages"].as_array().unwrap().len(), 1);
        assert_eq!(update["messages"][0]["text"], "更长的回复");
        assert_eq!(thread.page("since=expired")["reset"], true);
    }
}
