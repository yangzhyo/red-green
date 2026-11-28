use super::{
    antigravity,
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
    io::{self, BufReader, Read, Seek, SeekFrom},
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AntigravityReplyTarget {
    Tmux(String),
    Ghostty(String),
    AppleTerminal(String),
    Custom(String),
}
type TerminalLauncher = fn(&Path, &str, &Path) -> Result<ResumeTerminal, u16>;
type AntigravityReplyPrepare = fn(&str) -> Result<AntigravityReplyTarget, u16>;
type AntigravityReplyDispatch = fn(&AntigravityReplyTarget, &str) -> Result<(), u16>;
type AntigravityForegroundCheck = fn(u32, &str) -> crate::antigravity::ForegroundStatus;
pub struct LiveSource {
    snapshot: fn() -> Vec<Value>,
    status_dir: PathBuf,
    socket: PathBuf,
    selected: HashMap<String, Selection>,
    claude_titles: HashMap<String, JsonlFile<SessionTitle>>,
    antigravity_titles: HashMap<String, JsonlFile<antigravity::SessionTitle>>,
    stored: Option<StoredConversation>,
    history_directory: history::Directory,
    launch: TerminalLauncher,
    terminal_exists: fn(&ResumeTerminal) -> Result<bool, u16>,
    thread_terminal_exists: fn(&str, &[String]) -> Result<bool, u16>,
    launching: HashMap<String, ResumeTerminal>,
    antigravity_prepare: AntigravityReplyPrepare,
    antigravity_dispatch: AntigravityReplyDispatch,
    antigravity_foreground: AntigravityForegroundCheck,
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
    Claude(JsonlFile<Transcript>),
    Antigravity(JsonlFile<antigravity::Transcript>),
    Codex(CodexHistory),
}

#[derive(Default)]
struct JsonlFile<T> {
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

impl<T: Default> JsonlFile<T> {
    fn refresh(
        &mut self,
        path: &Path,
        consume_entries: impl FnOnce(BufReader<io::Take<File>>, &mut T) -> io::Result<usize>,
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
        let consumed = consume_entries(input, &mut self.data).map_err(|_| UNAVAILABLE)?;
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

// App Server returns complete turns, including turns without a visible user message.
fn turn_items(turn: &Value) -> impl Iterator<Item = (String, &'static str, String, bool)> {
    visible_items(turn)
        .into_iter()
        .enumerate()
        .map(|(index, (id, role, text))| (id, role, text, index == 0))
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
                        let rows: Vec<_> = turns.iter().rev().flat_map(turn_items).collect();
                        body.reconcile(
                            rows.iter()
                                .map(|(id, role, text, start)| {
                                    (id.as_str(), *role, text.as_str(), *start)
                                })
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
            for (id, role, text, start) in turns.iter().rev().flat_map(turn_items) {
                body.put_item(&id, role, text, start);
            }
        }
        // The newest turns may contain only tools or images. Fetch the first
        // readable segment before showing an empty document.
        if requested_older || body.is_empty() {
            seen.clear();
            cursor = self.older.clone();
            while let Some(token) = cursor.as_deref() {
                if !seen.insert(token.to_owned()) {
                    return Err(UNAVAILABLE);
                }
                let (turns, next) = self.read(socket, id, Some(token))?;
                let rows: Vec<_> = turns.iter().rev().flat_map(turn_items).collect();
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
            antigravity_titles: HashMap::new(),
            stored: None,
            history_directory: history::Directory::default(),
            launch: |_, _, _| Err(424),
            terminal_exists: |_| Err(503),
            thread_terminal_exists: |_, _| Err(503),
            launching: HashMap::new(),
            antigravity_prepare: |_| Err(424),
            antigravity_dispatch: |_, _| Err(424),
            antigravity_foreground: crate::antigravity::foreground_status,
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

    pub fn with_antigravity_reply(
        mut self,
        prepare: AntigravityReplyPrepare,
        dispatch: AntigravityReplyDispatch,
    ) -> Self {
        self.antigravity_prepare = prepare;
        self.antigravity_dispatch = dispatch;
        self
    }

    pub fn with_antigravity_foreground_check(
        mut self,
        check: AntigravityForegroundCheck,
    ) -> Self {
        self.antigravity_foreground = check;
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
        let mut seen = HashSet::new();
        (self.snapshot)()
            .into_iter()
            .filter_map(|session| {
                let sid = session["session_id"].as_str()?;
                let (key, source, path) = match session["source"].as_str() {
                    Some("codex") => {
                        if session["reader_has_turn"] == false {
                            return None;
                        }
                        sid.strip_prefix("codex-")?;
                        (sid.to_owned(), "Codex CLI", None)
                    }
                    Some("antigravity") => {
                        let cid = session["conversation_id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .or_else(|| {
                                self.mapping(&format!("antigravity-{sid}"))
                                    .is_some()
                                    .then_some(sid)
                            })?;
                        let key = format!("antigravity-{cid}");
                        if !seen.insert(key.clone()) {
                            return None;
                        }
                        let path = PathBuf::from(self.mapping(&key)?);
                        if !path.is_absolute() || !path.is_file() {
                            return None;
                        }
                        (key, "Antigravity CLI", Some(path))
                    }
                    _ => {
                        let path = PathBuf::from(self.mapping(sid)?);
                        if !path.is_absolute() || !path.is_file() {
                            return None;
                        }
                        (format!("claude-{sid}"), "Claude Code", Some(path))
                    }
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
            if file
                .refresh(path, |input, title| {
                    read_entries(sid, input, |row| title.insert(row))
                })
                .is_ok()
            {
                entry.title = file.data.get().to_owned();
            }
        } else if let Some(path) = entry
            .path
            .as_ref()
            .filter(|_| entry.key.starts_with("antigravity-") && entry.title == "未命名会话")
        {
            let file = self
                .antigravity_titles
                .entry(entry.key.clone())
                .or_default();
            if file
                .refresh(path, |input, title| {
                    antigravity::read_entries(input, |row| title.insert(row))
                })
                .is_ok()
            {
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
        if let Some(cid) = key.strip_prefix("antigravity-") {
            return self.send_antigravity_reply(key, cid, text, admit);
        }
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
        self.antigravity_titles
            .retain(|key, _| entries.iter().any(|row| row.key == *key));
        json!(entries
            .into_iter()
            .map(|entry| self.with_title(entry))
            .collect::<Vec<_>>())
    }
    fn messages(&mut self, key: &str, query: &str) -> Result<Value, u16> {
        self.current_messages(key, View::Nomad(query))
    }

    fn kindle_poll(&mut self, key: &str, since: &str, status_only: bool) -> Result<Value, u16> {
        self.current_messages(
            key,
            View::Kindle(&kindle::Position::poll(since, status_only)),
        )
    }

    fn kindle_messages(&mut self, key: &str, query: &str) -> Result<Value, u16> {
        self.current_messages(key, View::Kindle(&kindle::Position::parse(query)?))
    }
}

impl LiveSource {
    fn verify_antigravity_live_state(
        &mut self,
        key: &str,
        cid: &str,
        pid: u32,
        tty: &str,
    ) -> Result<(), u16> {
        use crate::antigravity::ForegroundStatus;
        match (self.antigravity_foreground)(pid, tty) {
            ForegroundStatus::Ready => (),
            ForegroundStatus::Backgrounded => return Err(409),
            ForegroundStatus::Gone => {
                self.selected.remove(key);
                return Err(410);
            }
        }
        let file = self.status_dir.join(format!("antigravity-pid-{pid}.json"));
        let Ok(raw) = fs::read_to_string(&file) else {
            self.selected.remove(key);
            return Err(410);
        };
        let Ok(fresh) = serde_json::from_str::<Value>(&raw) else {
            self.selected.remove(key);
            return Err(410);
        };
        if fresh["source"].as_str() != Some("antigravity")
            || fresh["conversation_id"].as_str() != Some(cid)
            || fresh["tty"].as_str() != Some(tty)
        {
            self.selected.remove(key);
            return Err(410);
        }
        match fresh["state"].as_str() {
            Some("idle" | "your_turn" | "completed" | "aborted") => Ok(()),
            Some("running" | "awaiting") => Err(409),
            _ => Err(UNAVAILABLE),
        }
    }

    fn send_antigravity_reply(
        &mut self,
        key: &str,
        cid: &str,
        text: &str,
        admit: impl FnOnce() -> Result<(), u16>,
    ) -> Result<(), u16> {
        let sanitized = crate::antigravity::validate_reply_text(text)?;
        let Some(session) = (self.snapshot)().into_iter().find(|s| {
            if s["source"].as_str() != Some("antigravity") {
                return false;
            }
            let session_cid = s["conversation_id"]
                .as_str()
                .filter(|v| !v.is_empty())
                .or_else(|| s["session_id"].as_str());
            session_cid == Some(cid)
        }) else {
            return Err(if self.mapping(key).is_some() {
                self.selected.remove(key);
                410
            } else {
                UNAVAILABLE
            });
        };
        if !matches!(
            self.selected.get(key),
            Some(Selection {
                history: History::Antigravity(_),
                ..
            })
        ) {
            return Err(412);
        }
        match session["state"].as_str() {
            Some("idle" | "your_turn" | "completed" | "aborted") => (),
            Some("running" | "awaiting") => return Err(409),
            _ => return Err(UNAVAILABLE),
        }
        let tty = session["tty"]
            .as_str()
            .filter(|t| t.starts_with("/dev/tty"))
            .ok_or(424u16)?;
        let pid = session["pid"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|&p| p > 0)
            .ok_or(424u16)?;

        self.verify_antigravity_live_state(key, cid, pid, tty)?;
        let target = (self.antigravity_prepare)(tty)?;
        // prepare（如 Ghostty OSC 7 探测）可能耗时数百毫秒，消费凭据前必须二次核验进程与会话状态。
        self.verify_antigravity_live_state(key, cid, pid, tty)?;
        admit()?;
        // 凭据已消费后不再重放；投递异常返回 502 由阅读端提示核对正文。
        (self.antigravity_dispatch)(&target, &sanitized).map_err(|_| 502u16)
    }

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
            if key.starts_with("antigravity-") && self.mapping(key).is_some() {
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
                history: if entry.key.starts_with("antigravity-") {
                    History::Antigravity(JsonlFile::default())
                } else if entry.path.is_some() {
                    History::Claude(JsonlFile::default())
                } else {
                    History::Codex(CodexHistory::default())
                },
            });
        let mut page = match &mut selection.history {
            History::Claude(file) => {
                let sid = key.strip_prefix("claude-").ok_or(UNAVAILABLE)?;
                refresh_jsonl_body(
                    file,
                    &entry.path.ok_or(UNAVAILABLE)?,
                    &mut selection.body,
                    view,
                    |input, transcript| read_entries(sid, input, |row| transcript.insert(row)),
                    Transcript::visible_with_turns,
                )
            }
            History::Antigravity(file) => refresh_jsonl_body(
                file,
                &entry.path.ok_or(UNAVAILABLE)?,
                &mut selection.body,
                view,
                |input, transcript| antigravity::read_entries(input, |row| transcript.insert(row)),
                antigravity::Transcript::visible_with_turns,
            ),
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

fn refresh_jsonl_body<T: Default>(
    file: &mut JsonlFile<T>,
    path: &Path,
    body: &mut Conversation,
    view: View,
    consume: impl FnOnce(BufReader<io::Take<File>>, &mut T) -> io::Result<usize>,
    rows: impl FnOnce(&T) -> Vec<(&str, &'static str, &str, bool)>,
) -> Result<Value, u16> {
    let change = file.refresh(path, consume)?;
    if matches!(change, FileChange::Replaced) {
        body.reconcile(vec![]);
    }
    if !matches!(change, FileChange::Unchanged) {
        body.reconcile(rows(&file.data));
    }
    view.render(body, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_SNAPSHOT: Mutex<Vec<Value>> = Mutex::new(Vec::new());

    fn test_snapshot() -> Vec<Value> {
        TEST_SNAPSHOT.lock().unwrap().clone()
    }

    #[test]
    fn antigravity_readable_session_streams_for_nomad_and_kindle_and_ends_with_410() {
        let dir = std::env::temp_dir().join(format!("rg-agy-reader-{}", rand::random::<u64>()));
        let reader_dir = dir.join(".reader");
        fs::create_dir_all(&reader_dir).unwrap();
        let transcript = dir.join("transcript_full.jsonl");
        fs::write(
            &transcript,
            format!(
                "{}\n{}\n",
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>\n实现 Issue 42\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nx\n</ADDITIONAL_METADATA>"}),
                json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","content":"第一轮答复"})
            ),
        )
        .unwrap();
        fs::write(
            reader_dir.join("antigravity-cid-1"),
            format!("{}\n", transcript.display()),
        )
        .unwrap();

        *TEST_SNAPSHOT.lock().unwrap() = vec![json!({
            "session_id": "pid-123",
            "source": "antigravity",
            "conversation_id": "cid-1",
            "project": "red-green",
            "state": "running",
        })];

        let mut source = LiveSource::new(test_snapshot, dir.clone(), dir.join("unused.sock"));
        let list = source.sessions();
        let rows = list.as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["key"], "antigravity-cid-1");
        assert_eq!(rows[0]["source"], "Antigravity CLI");
        assert_eq!(rows[0]["project"], "red-green");
        assert_eq!(rows[0]["state"], "running");
        assert_eq!(rows[0]["title"], "实现 Issue 42");

        let nomad = source.messages("antigravity-cid-1", "").unwrap();
        assert_eq!(nomad["messages"].as_array().unwrap().len(), 2);
        assert_eq!(nomad["messages"][0]["role"], "user");
        assert_eq!(nomad["messages"][0]["text"], "实现 Issue 42");
        assert_eq!(nomad["messages"][1]["text"], "第一轮答复");

        let kindle = source.kindle_messages("antigravity-cid-1", "").unwrap();
        assert_eq!(kindle["messages"].as_array().unwrap().len(), 2);
        assert_eq!(kindle["state"], "running");

        // 追加第二轮并模拟会话名更新
        fs::write(
            &transcript,
            format!(
                "{}{}\n{}\n",
                fs::read_to_string(&transcript).unwrap(),
                json!({"step_index":2,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>\n再加一行\n</USER_REQUEST>"}),
                json!({"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","content":"第二轮答复"})
            ),
        )
        .unwrap();
        *TEST_SNAPSHOT.lock().unwrap() = vec![json!({
            "session_id": "pid-123",
            "source": "antigravity",
            "pid": 123,
            "conversation_id": "cid-1",
            "title": "自定义标题",
            "project": "red-green",
            "tty": "/dev/ttys012",
            "state": "completed",
        })];
        let status_file = dir.join("antigravity-pid-123.json");
        fs::write(
            &status_file,
            json!({
                "session_id": "pid-123",
                "source": "antigravity",
                "pid": 123,
                "conversation_id": "cid-1",
                "tty": "/dev/ttys012",
                "state": "completed",
            })
            .to_string(),
        )
        .unwrap();

        let delta = source
            .messages(
                "antigravity-cid-1",
                &format!("since={}", nomad["since"].as_str().unwrap()),
            )
            .unwrap();
        assert_eq!(delta["messages"].as_array().unwrap().len(), 2);
        assert_eq!(delta["state"], "completed");
        assert_eq!(delta["title"], "自定义标题");

        static FG: Mutex<crate::antigravity::ForegroundStatus> =
            Mutex::new(crate::antigravity::ForegroundStatus::Ready);
        *FG.lock().unwrap() = crate::antigravity::ForegroundStatus::Ready;
        let mut source = source.with_antigravity_foreground_check(|_, _| *FG.lock().unwrap());

        // 未绑定终端投递回调时返回 424（不消费凭据）
        assert_eq!(
            source.send_reply("antigravity-cid-1", "hi", || panic!("424 前不应消费凭据")),
            Err(424)
        );
        static SENT: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
        static FAIL_DISPATCH: Mutex<bool> = Mutex::new(false);
        SENT.lock().unwrap().clear();
        *FAIL_DISPATCH.lock().unwrap() = false;
        let mut source = source.with_antigravity_reply(
            |tty| Ok(AntigravityReplyTarget::Custom(tty.to_owned())),
            |target, text| {
                if *FAIL_DISPATCH.lock().unwrap() {
                    return Err(502);
                }
                let AntigravityReplyTarget::Custom(tty) = target else {
                    return Err(424);
                };
                SENT.lock().unwrap().push((tty.clone(), text.to_owned()));
                Ok(())
            },
        );
        assert_eq!(
            source.send_reply("antigravity-cid-1", "坏\x1b[201~字符", || panic!("422 前不应消费")),
            Err(422)
        );
        // Ctrl-Z 挂起/后台时返回 409，不向宿主 shell 粘贴
        *FG.lock().unwrap() = crate::antigravity::ForegroundStatus::Backgrounded;
        assert_eq!(
            source.send_reply("antigravity-cid-1", "挂起态", || panic!("409 前不应消费")),
            Err(409)
        );
        *FG.lock().unwrap() = crate::antigravity::ForegroundStatus::Ready;

        // 正常投递
        assert_eq!(
            source.send_reply("antigravity-cid-1", "第一行\r\n第二行", || Ok(())),
            Ok(())
        );
        assert_eq!(
            *SENT.lock().unwrap(),
            vec![("/dev/ttys012".into(), "第一行\n第二行".into())]
        );

        // 投递失败时返回 502（凭据已由 admit 消费）
        *FAIL_DISPATCH.lock().unwrap() = true;
        let mut admitted = false;
        assert_eq!(
            source.send_reply("antigravity-cid-1", "可能失败", || {
                admitted = true;
                Ok(())
            }),
            Err(502)
        );
        assert!(admitted);
        *FAIL_DISPATCH.lock().unwrap() = false;

        // 磁盘状态文件在发送前翻转为 awaiting 时，即使快照未刷新也拒绝 409
        fs::write(
            &status_file,
            json!({
                "session_id": "pid-123",
                "source": "antigravity",
                "pid": 123,
                "conversation_id": "cid-1",
                "tty": "/dev/ttys012",
                "state": "awaiting",
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            source.send_reply("antigravity-cid-1", "等待审批", || panic!("409 前不应消费")),
            Err(409)
        );

        // 状态文件切会话（/clear）时返回 410
        fs::write(
            &status_file,
            json!({
                "session_id": "pid-123",
                "source": "antigravity",
                "pid": 123,
                "conversation_id": "",
                "tty": "/dev/ttys012",
                "state": "idle",
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            source.send_reply("antigravity-cid-1", "已清空", || panic!("410 前不应消费")),
            Err(410)
        );

        // 会话退出后返回 410
        TEST_SNAPSHOT.lock().unwrap().clear();
        assert_eq!(source.messages("antigravity-cid-1", ""), Err(410));
        assert_eq!(source.send_reply("antigravity-cid-1", "hi", || Ok(())), Err(410));
        assert_eq!(source.messages("antigravity-unknown", ""), Err(UNAVAILABLE));

        let _ = fs::remove_dir_all(dir);
    }
}
