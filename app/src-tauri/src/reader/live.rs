use super::{claude::Transcript, codex::visible_items, Conversation, Source};
use crate::codex::rpc::Rpc;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufReader, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const UNAVAILABLE: u16 = 503;
pub struct LiveSource {
    snapshot: fn() -> Vec<Value>,
    status_dir: PathBuf,
    socket: PathBuf,
    selected: HashMap<String, Selection>,
}
#[derive(serde::Serialize)]
struct DirectoryEntry {
    key: String,
    project: String,
    source: &'static str,
    state: Option<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

struct Selection {
    body: Conversation,
    history: History,
}
enum History {
    Claude(ClaudeFile),
    Codex(CodexHistory),
}

#[derive(Default)]
struct ClaudeFile {
    transcript: Transcript,
    identity: (u64, u64),
    offset: u64,
    modified: Option<std::time::SystemTime>,
}

impl ClaudeFile {
    fn refresh(&mut self, path: &Path, sid: &str, body: &mut Conversation) -> Result<(), u16> {
        let mut file = File::open(path).map_err(|_| UNAVAILABLE)?;
        let meta = file.metadata().map_err(|_| UNAVAILABLE)?;
        let identity = (meta.dev(), meta.ino());
        let modified = meta.modified().ok();
        if identity != self.identity
            || meta.len() < self.offset
            || (meta.len() == self.offset && self.modified != modified)
        {
            body.reconcile(vec![]);
            *self = Self {
                identity,
                ..Self::default()
            };
        }
        if self.offset == meta.len() {
            return Ok(());
        }
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|_| UNAVAILABLE)?;
        let consumed = self
            .transcript
            .append(sid, BufReader::new(file))
            .map_err(|_| UNAVAILABLE)?;
        self.offset += consumed as u64;
        self.modified = modified;
        if consumed > 0 {
            body.reconcile(self.transcript.visible());
        }
        Ok(())
    }
}

#[derive(Default)]
struct CodexHistory {
    rpc: Option<Rpc>,
    latest_turn: Option<String>,
    older: Option<String>,
}
impl CodexHistory {
    fn read(
        &mut self,
        socket: &Path,
        id: &str,
        cursor: Option<&str>,
    ) -> Result<(Vec<Value>, Option<String>), u16> {
        if self.rpc.is_none() {
            let mut rpc = Rpc::connect(socket).map_err(|_| UNAVAILABLE)?;
            rpc.watch_for_close(id);
            self.rpc = Some(rpc);
        }
        let rpc = self.rpc.as_mut().unwrap();
        let params = json!({"threadId":id,"cursor":cursor,"limit":16,"sortDirection":"desc","itemsView":"full"});
        let mut result = rpc.call("thread/turns/list", params.clone());
        if rpc.account_changed && self.latest_turn.is_none() {
            // App Server announces the initial account after initialization. Discard the
            // overlapping response and establish a fresh baseline on this same connection.
            rpc.account_changed = false;
            result = rpc.call("thread/turns/list", params);
        }
        if rpc.account_changed {
            self.rpc = None;
            return Err(409);
        }
        if rpc.thread_closed {
            return Err(410);
        }
        let page = result.map_err(|_| {
            self.rpc = None;
            UNAVAILABLE
        })?;
        let turns = page["data"].as_array().ok_or(UNAVAILABLE)?.clone();
        Ok((turns, page["nextCursor"].as_str().map(str::to_owned)))
    }

    fn refresh(
        &mut self,
        socket: &Path,
        id: &str,
        query: &str,
        body: &mut Conversation,
    ) -> Result<Value, u16> {
        let mut cursor = None;
        let mut seen = HashSet::new();
        if body.needs_history(query) && self.older.is_some() {
            cursor = self.older.clone();
            while let Some(token) = cursor.as_deref() {
                if !seen.insert(token.to_owned()) {
                    return Err(UNAVAILABLE);
                }
                let (turns, next) = self.read(socket, id, Some(token))?;
                let rows: Vec<_> = turns.iter().rev().flat_map(visible_items).collect();
                cursor = next;
                if !rows.is_empty() {
                    body.prepend(rows);
                    break;
                }
            }
            self.older = cursor;
        } else {
            // Read newest first, stopping at the last observed turn. Include it: its final reply can still grow.
            let mut turns = Vec::new();
            loop {
                let (batch, next) = self.read(socket, id, cursor.as_deref())?;
                let mut overlap = false;
                for turn in batch {
                    overlap = self.latest_turn.is_some()
                        && turn["id"].as_str() == self.latest_turn.as_deref();
                    turns.push(turn);
                    if overlap {
                        break;
                    }
                }
                if self.latest_turn.is_none() {
                    self.older = next;
                    break;
                }
                if overlap {
                    break;
                }
                let Some(next) = next else {
                    if !turns.is_empty() {
                        let rows: Vec<_> = turns.iter().rev().flat_map(visible_items).collect();
                        body.reconcile(
                            rows.iter()
                                .map(|(id, role, text)| (id.as_str(), *role, text.as_str()))
                                .collect(),
                        );
                        self.older = None;
                    }
                    break;
                };
                if !seen.insert(next.clone()) {
                    return Err(UNAVAILABLE);
                }
                cursor = Some(next);
            }
            if let Some(head) = turns.first().and_then(|t| t["id"].as_str()) {
                self.latest_turn = Some(head.to_owned());
            }
            for (id, role, text) in turns.iter().rev().flat_map(visible_items) {
                body.put(&id, role, text);
            }
        }
        Ok(body.history_page(query, self.older.is_some()))
    }
}

impl LiveSource {
    pub fn new(snapshot: fn() -> Vec<Value>, status_dir: PathBuf, socket: PathBuf) -> Self {
        Self {
            snapshot,
            status_dir,
            socket,
            selected: HashMap::new(),
        }
    }

    fn mapping(&self, id: &str) -> Option<String> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        {
            return None;
        }
        fs::read_to_string(self.status_dir.join(".reader").join(id))
            .ok()
            .map(|s| s.trim_end_matches('\n').into())
    }

    fn directory(&self) -> Vec<DirectoryEntry> {
        (self.snapshot)()
            .into_iter()
            .filter_map(|session| {
                let sid = session["session_id"].as_str()?;
                let (key, source, path) = if session["source"] == "codex" {
                    if session["reader_has_turn"] == false {
                        return None;
                    }
                    sid.strip_prefix("codex-")?;
                    (sid.to_owned(), "Codex CLI", None)
                } else {
                    let path = PathBuf::from(self.mapping(sid)?);
                    if !path.is_absolute() || !path.is_file() {
                        return None;
                    }
                    (format!("claude-{sid}"), "Claude Code", Some(path))
                };
                Some(DirectoryEntry {
                    key,
                    source,
                    path,
                    project: session["project"].as_str().unwrap_or("?").to_owned(),
                    state: session["state"].as_str().map(str::to_owned),
                })
            })
            .collect()
    }
}

impl Source for LiveSource {
    fn send_reply(
        &mut self,
        key: &str,
        text: &str,
        admit: impl FnOnce() -> Result<(), u16>,
    ) -> Result<(), u16> {
        let id = key.strip_prefix("codex-").ok_or(405u16)?;
        if !self.directory().iter().any(|entry| entry.key == key) {
            return Err(410);
        }
        let Some(Selection {
            history: History::Codex(history),
            ..
        }) = self.selected.get_mut(key)
        else {
            return Err(412);
        };
        let rpc = history.rpc.as_mut().ok_or(UNAVAILABLE)?;
        let info = rpc
            .call("thread/read", json!({"threadId":id, "includeTurns":false}))
            .map_err(|_| UNAVAILABLE)?;
        if rpc.account_changed || rpc.thread_closed || info["thread"]["id"] != id {
            return Err(410);
        }
        match info["thread"]["status"]["type"].as_str() {
            Some("idle") => (),
            Some("active") => return Err(409),
            _ => return Err(UNAVAILABLE),
        }
        admit()?;
        // Leave the Mac terminal subscribed and responsible for approval requests.
        // Once dispatched, a transport failure is ambiguous. Never replay turn/start.
        let result = rpc
            .call(
                "turn/start",
                json!({"threadId":id, "input":[{"type":"text","text":text,"text_elements":[]}]}),
            )
            .map_err(|_| 502u16)?;
        result["turn"]["id"].as_str().map(|_| ()).ok_or(502)
    }

    fn sessions(&mut self) -> Value {
        json!(self.directory())
    }
    fn messages(&mut self, key: &str, query: &str) -> Result<Value, u16> {
        let Some(entry) = self.directory().into_iter().find(|row| row.key == key) else {
            if key
                .strip_prefix("claude-")
                .and_then(|id| self.mapping(id))
                .as_deref()
                == Some("ended")
            {
                self.selected.remove(key);
                return Err(410);
            }
            if let Some(Selection {
                history: History::Codex(history),
                ..
            }) = self.selected.get_mut(key)
            {
                if let Some(rpc) = history.rpc.as_mut() {
                    // Drain pending notifications without reading an unlisted thread's content.
                    let ping = rpc.call("thread/loaded/list", json!({"limit":1}));
                    let status = if rpc.account_changed {
                        409
                    } else if rpc.thread_closed {
                        410
                    } else {
                        UNAVAILABLE
                    };
                    if status != UNAVAILABLE {
                        self.selected.remove(key);
                        return Err(status);
                    }
                    if ping.is_err() {
                        history.rpc = None;
                    }
                }
            }
            return Err(UNAVAILABLE);
        };
        let selection = self
            .selected
            .entry(key.to_owned())
            .or_insert_with(|| Selection {
                body: Conversation::new(key, &entry.project, entry.source),
                history: if entry.path.is_some() {
                    History::Claude(ClaudeFile::default())
                } else {
                    History::Codex(CodexHistory::default())
                },
            });
        let mut page = match &mut selection.history {
            History::Claude(file) => {
                file.refresh(
                    &entry.path.ok_or(UNAVAILABLE)?,
                    key.strip_prefix("claude-").ok_or(UNAVAILABLE)?,
                    &mut selection.body,
                )?;
                Ok(selection.body.page(query))
            }
            History::Codex(history) => {
                let result = history.refresh(
                    &self.socket,
                    key.strip_prefix("codex-").ok_or(UNAVAILABLE)?,
                    query,
                    &mut selection.body,
                );
                if matches!(result, Err(409 | 410)) {
                    self.selected.remove(key);
                }
                result
            }
        }?;
        page["state"] = json!(entry.state);
        Ok(page)
    }
}
