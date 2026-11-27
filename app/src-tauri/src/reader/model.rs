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

#[derive(Clone)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub order: i64,
    pub(super) starts_turn: bool,
    version: u64,
}

impl Message {
    fn for_reader(&self) -> Value {
        json!({ "id": self.id, "role": self.role, "text": self.text, "order": self.order,
            "html": super::markdown::render(&self.text) })
    }
}

pub struct Conversation {
    listing: Listing,
    messages: Vec<Message>,
    version: u64,
    generation: u64,
}

#[derive(Clone, Copy)]
pub(super) enum View<'a> {
    Nomad(&'a str),
    Kindle(&'a super::kindle::Position),
}

impl View<'_> {
    pub fn needs_history(self, body: &Conversation) -> bool {
        match self {
            Self::Nomad(query) => body.needs_history(query),
            Self::Kindle(position) => position.needs_older(&body.messages),
        }
    }

    pub fn render(self, body: &Conversation, older: bool) -> Result<Value, u16> {
        match self {
            Self::Nomad(query) => Ok(body.history_page(query, older)),
            Self::Kindle(pos) => body.kindle_page(pos, older),
        }
    }
}

impl Conversation {
    pub(super) fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub(super) fn kindle_page(
        &self,
        pos: &super::kindle::Position,
        older: bool,
    ) -> Result<Value, u16> {
        super::kindle::page(&self.messages, pos, older, &self.cursor(self.version))
    }

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
        self.put_item(id, role, text, role == "user");
    }

    pub(super) fn put_item(&mut self, id: &str, role: &str, text: String, starts_turn: bool) {
        if let Some(message) = self.messages.iter_mut().find(|m| m.id == id) {
            if message.text != text || message.starts_turn != starts_turn {
                message.starts_turn = starts_turn;
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
            starts_turn,
            text,
            order: self.messages.last().map_or(0, |m| m.order + 1),
            version: self.version,
        });
    }

    fn cursor(&self, value: impl std::fmt::Display) -> String {
        format!("{:x}-{value}", self.generation)
    }

    fn position<T: std::str::FromStr>(&self, cursor: &str) -> Option<T> {
        let (generation, value) = cursor.split_once('-')?;
        (u64::from_str_radix(generation, 16).ok()? == self.generation)
            .then(|| value.parse().ok())
            .flatten()
    }

    pub(super) fn needs_history(&self, query: &str) -> bool {
        query
            .strip_prefix("before=")
            .and_then(|s| self.position::<i64>(s))
            .is_some_and(|p| p == self.messages.first().map_or(0, |m| m.order))
    }

    pub(super) fn prepend(&mut self, rows: Vec<(String, &'static str, String, bool)>) {
        let start = self.messages.first().map_or(0, |m| m.order) - rows.len() as i64;
        let mut earlier: Vec<_> = rows
            .into_iter()
            .enumerate()
            .map(|(i, (id, role, text, starts_turn))| Message {
                starts_turn,
                id,
                role: role.into(),
                text,
                order: start + i as i64,
                version: 0,
            })
            .collect();
        earlier.append(&mut self.messages);
        self.messages = earlier;
    }

    pub(super) fn reconcile(&mut self, rows: Vec<(&str, &'static str, &str, bool)>) {
        if self.messages.len() > rows.len()
            || self.messages.iter().zip(&rows).any(|(m, r)| m.id != r.0)
        {
            self.generation = rand::random();
            self.messages.clear();
        }
        for (i, (id, role, text, starts_turn)) in rows.into_iter().enumerate() {
            if let Some(msg) = self.messages.get_mut(i) {
                if msg.text != text || msg.starts_turn != starts_turn {
                    msg.starts_turn = starts_turn;
                    self.version += 1;
                    msg.version = self.version;
                    msg.text = text.into();
                }
            } else {
                self.version += 1;
                self.messages.push(Message {
                    starts_turn,
                    id: id.into(),
                    role: role.into(),
                    text: text.into(),
                    order: i as i64,
                    version: self.version,
                });
            }
        }
    }

    pub(super) fn page(&self, query: &str) -> Value {
        self.history_page(query, false)
    }
    pub(super) fn history_page(&self, query: &str, has_older: bool) -> Value {
        let since = query
            .strip_prefix("since=")
            .and_then(|s| self.position::<u64>(s))
            .filter(|&v| v <= self.version);
        if let Some(version) = since {
            let messages: Vec<_> = self
                .messages
                .iter()
                .filter(|m| m.version > version)
                .map(Message::for_reader)
                .collect();
            return json!({"messages": messages, "since":self.cursor(self.version)});
        }
        let before = query
            .strip_prefix("before=")
            .and_then(|s| self.position::<i64>(s))
            .map(|order| self.messages.partition_point(|m| m.order < order));
        let end = before.unwrap_or(self.messages.len());
        let start = end.saturating_sub(PAGE);
        let messages: Vec<_> = self.messages[start..end]
            .iter()
            .map(Message::for_reader)
            .collect();
        json!({"messages":messages, "before": (start > 0 || has_older).then(||self.cursor(self.messages.get(start).map_or(0, |m| m.order))),
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
