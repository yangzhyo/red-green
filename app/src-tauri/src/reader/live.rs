use super::{
    claude::{read_entries, SessionTitle, Transcript},
    codex::visible_items,
    history, kindle,
    model::View,
    Conversation, Source,
};
use crate::codex::{model::thread_title, rpc::Rpc};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufReader, Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(super) const UNAVAILABLE: u16 = 503;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResumeTerminal {
    Ghostty(String),
    Terminal { tty: String, inode: u64 },
    Unknown,
}
type TerminalLauncher = fn(&Path, &str, &Path) -> Result<ResumeTerminal, u16>;
pub struct LiveSource {
    snapshot: fn() -> Vec<Value>,
    status_dir: PathBuf,
    socket: PathBuf,
    selected: HashMap<String, Selection>,
    claude_titles: HashMap<String, ClaudeFile<SessionTitle>>,
    stored: Option<StoredConversation>,
    history_directory: history::Directory,
    launch: TerminalLauncher,
    terminal_exists: fn(&ResumeTerminal) -> Result<bool, u16>,
    thread_terminal_exists: fn(&str, &[String]) -> Result<bool, u16>,
    launching: HashMap<String, ResumeTerminal>,
}

struct StoredConversation {
    id: String,
    body: Conversation,
    history: CodexHistory,
}
#[derive(serde::Serialize)]
struct DirectoryEntry {
    key: String,
    project: String,
    source: &'static str,
    title: String,
    state: Option<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

struct Selection {
    body: Conversation,
    history: History,
}
enum History {
    Claude(ClaudeFile<Transcript>),
    Codex(CodexHistory),
}

#[derive(Default)]
struct ClaudeFile<T> {
    data: T,
    identity: (u64, u64),
    offset: u64,
    len: u64,
    modified: Option<std::time::SystemTime>,
}

enum FileChange {
    Unchanged,
    Appended,
    Replaced,
}

impl<T: Default> ClaudeFile<T> {
    fn refresh(
        &mut self,
        path: &Path,
        sid: &str,
        insert: fn(&mut T, &Value),
    ) -> Result<FileChange, u16> {
        let mut file = File::open(path).map_err(|_| UNAVAILABLE)?;
        let meta = file.metadata().map_err(|_| UNAVAILABLE)?;
        let identity = (meta.dev(), meta.ino());
        let modified = meta.modified().ok();
        let replaced = identity != self.identity
            || meta.len() < self.len
            || (meta.len() == self.len && self.modified != modified);
        if replaced {
            *self = Self {
                identity,
                ..Self::default()
            };
        } else if self.len == meta.len() {
            return Ok(FileChange::Unchanged);
        }
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|_| UNAVAILABLE)?;
        let input = BufReader::new(file.take(meta.len() - self.offset));
        let consumed =
            read_entries(sid, input, |row| insert(&mut self.data, row)).map_err(|_| UNAVAILABLE)?;
        self.offset += consumed as u64;
        self.len = meta.len();
        self.modified = modified;
        Ok(if replaced {
            FileChange::Replaced
        } else if consumed > 0 {
            FileChange::Appended
        } else {
            FileChange::Unchanged
        })
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
        let new_connection = self.rpc.is_none();
        if new_connection {
            let mut rpc = Rpc::connect(socket).map_err(|_| UNAVAILABLE)?;
            rpc.watch_for_close(id);
            self.rpc = Some(rpc);
        }
        let rpc = self.rpc.as_mut().unwrap();
        let params = json!({"threadId":id,"cursor":cursor,"limit":16,"sortDirection":"desc","itemsView":"full"});
        let mut result = rpc.call("thread/turns/list", params.clone());
        if rpc.account_changed && new_connection && self.latest_turn.is_none() {
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
        view: View,
        body: &mut Conversation,
    ) -> Result<Value, u16> {
        let mut cursor = None;
        let mut seen = HashSet::new();
        let requested_older = view.needs_history(body) && self.older.is_some();
        if !requested_older {
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
        // The newest turns may contain only tools or images. Fetch the first
        // readable segment before showing an empty Kindle document.
        if requested_older || (matches!(view, View::Kindle(_)) && body.is_empty()) {
            seen.clear();
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
        }
        view.render(body, self.older.is_some())
    }
}

impl LiveSource {
    pub fn new(snapshot: fn() -> Vec<Value>, status_dir: PathBuf, socket: PathBuf) -> Self {
        Self {
            snapshot,
            status_dir,
            socket,
            selected: HashMap::new(),
            claude_titles: HashMap::new(),
            stored: None,
            history_directory: history::Directory::default(),
            launch: |_, _, _| Err(424),
            terminal_exists: |_| Err(503),
            thread_terminal_exists: |_, _| Err(503),
            launching: HashMap::new(),
        }
    }

    pub fn with_terminal_launcher(
        mut self,
        launch: TerminalLauncher,
        terminal_exists: fn(&ResumeTerminal) -> Result<bool, u16>,
        thread_terminal_exists: fn(&str, &[String]) -> Result<bool, u16>,
    ) -> Self {
        self.launch = launch;
        self.terminal_exists = terminal_exists;
        self.thread_terminal_exists = thread_terminal_exists;
        self
    }

    fn bound_terminal(&self, key: &str) -> Option<Value> {
        (self.snapshot)().into_iter().find(|session| {
            session["source"] == "codex"
                && session["session_id"] == key
                && session["tty"].as_str().is_some_and(|tty| !tty.is_empty())
        })
    }

    fn ready_terminal(&self, key: &str) -> Option<Value> {
        self.bound_terminal(key)
            .filter(|session| session["reader_has_turn"] != false)
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
                    title: session["title"]
                        .as_str()
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or("未命名会话")
                        .to_owned(),
                })
            })
            .collect()
    }

    fn with_title(&mut self, mut entry: DirectoryEntry) -> DirectoryEntry {
        if let (Some(path), Some(sid)) = (&entry.path, entry.key.strip_prefix("claude-")) {
            let file = self.claude_titles.entry(entry.key.clone()).or_default();
            if file.refresh(path, sid, SessionTitle::insert).is_ok() {
                entry.title = file.data.get().to_owned();
            }
        }
        entry
    }
}

impl Source for LiveSource {
    fn history_entry(&mut self, key: &str) -> Result<Value, u16> {
        let id = history::thread_id(key)?;
        let mut rpc = Rpc::connect(&self.socket).map_err(|_| UNAVAILABLE)?;
        let thread = history::find(&mut rpc, id, true)?;
        let mut entry = history::list_entry(&thread).ok_or(UNAVAILABLE)?;
        if let Some(session) = self.ready_terminal(key) {
            entry["state"] = session["state"].clone();
        }
        Ok(entry)
    }

    fn history(&mut self, cursor: Option<&str>) -> Result<Value, u16> {
        self.history_directory.list(&self.socket, cursor)
    }

    fn history_status(&mut self, generation: &str) -> Result<Value, u16> {
        self.history_directory.status(generation)
    }

    fn history_messages(&mut self, key: &str, query: &str) -> Result<Value, u16> {
        let id = history::thread_id(key)?;
        // 一次只保留正在阅读的一份正文和连接。权限变化、断线或切换会话即丢弃。
        let stored = self.stored.take().filter(|s| s.id == id);
        let fresh = stored.is_none();
        let mut stored = match stored {
            Some(stored) => stored,
            None => StoredConversation {
                id: id.into(),
                body: Conversation::new(key, "", "Codex CLI"),
                history: CodexHistory {
                    rpc: Some(Rpc::connect(&self.socket).map_err(|_| UNAVAILABLE)?),
                    ..Default::default()
                },
            },
        };
        let rpc = stored.history.rpc.as_mut().ok_or(UNAVAILABLE)?;
        let thread = history::find(rpc, id, fresh)?;
        let mut page =
            stored
                .history
                .refresh(&self.socket, id, View::Nomad(query), &mut stored.body)?;
        let terminal = self.ready_terminal(key);
        page["live"] = terminal.is_some().into();
        page["title"] = thread_title(&thread).into();
        if let Some(session) = terminal {
            page["state"] = session["state"].clone();
            self.launching.remove(id);
        }
        self.stored = Some(stored);
        Ok(page)
    }

    fn resume(&mut self, key: &str, admit: impl FnOnce() -> Result<(), u16>) -> Result<Value, u16> {
        let id = history::thread_id(key)?;
        let mut rpc = Rpc::connect(&self.socket).map_err(|_| UNAVAILABLE)?;
        history::find(&mut rpc, id, true)?;
        if let Some(session) = self.bound_terminal(key) {
            let ready = session["reader_has_turn"] != false;
            admit()?;
            if ready {
                self.launching.remove(id);
            }
            return Ok(json!({"ready":ready}));
        }
        if let Some(terminal) = self.launching.get(id) {
            match (self.terminal_exists)(terminal) {
                Ok(true) => {
                    admit()?;
                    return Ok(json!({"ready":false}));
                }
                Ok(false) => {
                    self.launching.remove(id);
                }
                // Apple Event may have succeeded without returning a native handle.
                // Check the exact thread title before asking for recovery on the Mac.
                Err(428) => (),
                Err(status) => return Err(status),
            }
        }
        let loaded = history::loaded_threads(&mut rpc)?;
        if loaded.iter().any(|thread| thread == id) && (self.thread_terminal_exists)(id, &loaded)? {
            admit()?;
            return Ok(json!({"ready":false}));
        }
        if self.launching.contains_key(id) {
            return Err(428);
        }
        // 查询原生终端可能较慢，在打开标签前再次取得当前账号下的原目录。
        let thread = history::find(&mut rpc, id, false)?;
        let cwd = thread["cwd"]
            .as_str()
            .map(Path::new)
            .filter(|path| path.is_absolute() && path.is_dir())
            .ok_or(422u16)?;
        admit()?;
        let tab = (self.launch)(cwd, id, &self.socket)?;
        self.launching.insert(id.into(), tab);
        Ok(json!({"ready":false}))
    }

    fn send_reply(
        &mut self,
        key: &str,
        text: &str,
        admit: impl FnOnce() -> Result<(), u16>,
    ) -> Result<(), u16> {
        let id = key.strip_prefix("codex-").ok_or(405u16)?;
        if !self.directory().iter().any(|entry| entry.key == key) {
            return Err(UNAVAILABLE);
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
        if rpc.account_changed || info["thread"]["id"] != id {
            return Err(412);
        }
        if rpc.thread_closed {
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
        let response = rpc
            .call_response(
                "turn/start",
                json!({"threadId":id, "input":[{"type":"text","text":text,"text_elements":[]}]}),
            )
            .map_err(|_| 502u16)?;
        if matches!(
            response["error"]["code"].as_i64(),
            Some(-32600 | -32601 | -32602 | -32001)
        ) {
            return Err(424);
        }
        response["result"]["turn"]["id"]
            .as_str()
            .map(|_| ())
            .ok_or(502)
    }

    fn sessions(&mut self) -> Value {
        let entries = self.directory();
        self.claude_titles
            .retain(|key, _| entries.iter().any(|row| row.key == *key));
        json!(entries
            .into_iter()
            .map(|entry| self.with_title(entry))
            .collect::<Vec<_>>())
    }
    fn messages(&mut self, key: &str, query: &str) -> Result<Value, u16> {
        self.current_messages(key, View::Nomad(query))
    }

    fn kindle_messages(&mut self, key: &str, query: &str) -> Result<Value, u16> {
        self.current_messages(key, View::Kindle(&kindle::Position::parse(query)?))
    }
}

impl LiveSource {
    fn current_messages(&mut self, key: &str, view: View) -> Result<Value, u16> {
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
        let entry = self.with_title(entry);
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
                let change = file.refresh(
                    &entry.path.ok_or(UNAVAILABLE)?,
                    key.strip_prefix("claude-").ok_or(UNAVAILABLE)?,
                    Transcript::insert,
                )?;
                if matches!(change, FileChange::Replaced) {
                    selection.body.reconcile(vec![]);
                }
                if !matches!(change, FileChange::Unchanged) {
                    selection.body.reconcile(file.data.visible());
                }
                view.render(&selection.body, false)
            }
            History::Codex(history) => {
                let result = history.refresh(
                    &self.socket,
                    key.strip_prefix("codex-").ok_or(UNAVAILABLE)?,
                    view,
                    &mut selection.body,
                );
                if matches!(result, Err(409 | 410)) {
                    self.selected.remove(key);
                }
                result
            }
        }?;
        page["state"] = json!(entry.state);
        page["title"] = entry.title.into();
        Ok(page)
    }
}
