use super::{claude::Transcript, codex::visible_items, Conversation, Source};
use crate::codex::rpc::Rpc;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
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
    selected: Option<Selection>,
}
struct Selection {
    key: String,
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
            self.rpc = Some(Rpc::connect(socket).map_err(|_| UNAVAILABLE)?);
        }
        let rpc = self.rpc.as_mut().unwrap();
        let result = rpc.call("thread/turns/list",json!({"threadId":id,"cursor":cursor,"limit":16,"sortDirection":"desc","itemsView":"full"}));
        if rpc.account_changed {
            self.rpc = None;
            return Err(409);
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
            selected: None,
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

    fn directory(&self) -> Vec<(String, String, String, Option<PathBuf>)> {
        (self.snapshot)()
            .into_iter()
            .filter_map(|s| {
                let sid = s["session_id"].as_str()?;
                let project = s["project"].as_str().unwrap_or("?").to_owned();
                if s["source"] == "codex" {
                    let id = sid.strip_prefix("codex-")?;
                    Some((format!("codex-{id}"), project, "Codex CLI".into(), None))
                } else {
                    let path = PathBuf::from(self.mapping(sid)?);
                    if !path.is_absolute() || !path.is_file() {
                        return None;
                    }
                    Some((
                        format!("claude-{sid}"),
                        project,
                        "Claude Code".into(),
                        Some(path),
                    ))
                }
            })
            .collect()
    }
}

impl Source for LiveSource {
    fn sessions(&mut self) -> Value {
        json!(self
            .directory()
            .into_iter()
            .map(
                |(key, project, source, _)| json!({ "key":key, "project":project, "source":source })
            )
            .collect::<Vec<_>>())
    }
    fn messages(&mut self, key: &str, query: &str) -> Result<Value, u16> {
        let Some((_, project, source, path)) =
            self.directory().into_iter().find(|row| row.0 == key)
        else {
            if key
                .strip_prefix("claude-")
                .and_then(|id| self.mapping(id))
                .as_deref()
                == Some("ended")
            {
                self.selected = None;
                return Err(410);
            }
            return Err(UNAVAILABLE);
        };
        if self.selected.as_ref().is_none_or(|s| s.key != key) {
            self.selected = Some(Selection {
                key: key.into(),
                body: Conversation::new(key, &project, &source),
                history: if path.is_some() {
                    History::Claude(ClaudeFile::default())
                } else {
                    History::Codex(CodexHistory::default())
                },
            });
        }
        let selection = self.selected.as_mut().unwrap();
        match &mut selection.history {
            History::Claude(file) => {
                file.refresh(
                    &path.ok_or(UNAVAILABLE)?,
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
                if matches!(result, Err(409)) {
                    self.selected = None;
                }
                result
            }
        }
    }
}
