use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead};

/// Count complete lines so a growing file can resume before its unfinished tail.
pub(super) fn read_entries(
    session: &str,
    mut input: impl BufRead,
    mut insert: impl FnMut(&Value),
) -> io::Result<usize> {
    let mut consumed = 0;
    let mut line = Vec::new();
    loop {
        line.clear();
        let size = input.read_until(b'\n', &mut line)?;
        if line.last() != Some(&b'\n') {
            break;
        }
        consumed += size;
        if let Ok(row) = serde_json::from_slice::<Value>(&line) {
            if row["sessionId"].as_str() == Some(session) && row["isSidechain"] != true {
                insert(&row);
            }
        }
    }
    Ok(consumed)
}

#[derive(Default)]
pub(super) struct SessionTitle {
    custom: Option<String>,
    generated: Option<String>,
    first_prompt: Option<String>,
}

impl SessionTitle {
    pub(super) fn get(&self) -> &str {
        self.custom
            .as_deref()
            .or(self.generated.as_deref())
            .or(self.first_prompt.as_deref())
            .unwrap_or("未命名会话")
    }

    pub(super) fn insert(&mut self, row: &Value) {
        let short = |s: &str| {
            s.lines()
                .map(str::trim)
                .find(|s| !s.is_empty())
                .map(|line| line.chars().take(120).collect::<String>())
        };
        match row["type"].as_str() {
            Some("custom-title") => self.custom = row["customTitle"].as_str().and_then(short),
            Some("ai-title") => self.generated = row["aiTitle"].as_str().and_then(short),
            Some("user") if self.first_prompt.is_none() => {
                let prompt = visible_message(row, |s| {
                    ![
                        "<local-command-",
                        "<command-name>",
                        "<command-message>",
                        "<command-args>",
                        "<task-notification>",
                        "[Request interrupted by user",
                    ]
                    .iter()
                    .any(|prefix| s.starts_with(prefix))
                });
                self.first_prompt = prompt.and_then(|(_, text)| short(&text));
            }
            _ => (),
        }
    }
}

struct Record {
    parent: Option<String>,
    body: Option<(&'static str, String)>,
    starts_turn: bool,
}

#[derive(Default)]
pub struct Transcript {
    records: HashMap<String, Record>,
    leaf: Option<String>,
}

impl Transcript {
    pub fn append(&mut self, session: &str, input: impl BufRead) -> io::Result<usize> {
        read_entries(session, input, |row| self.insert(row))
    }
    pub(super) fn insert(&mut self, row: &Value) {
        let Some(id) = row["uuid"].as_str() else {
            return;
        };
        let body = visible_message(row, |_| true);
        let parent = row["parentUuid"].as_str().map(str::to_owned);
        let attachment_prompt = row["type"] == "user"
            && row["message"]["role"] == "user"
            && row["isMeta"] != true
            && row["isCompactSummary"] != true
            && row["message"]["content"].as_array().is_some_and(|blocks| {
                blocks
                    .iter()
                    .any(|b| matches!(b["type"].as_str(), Some("image" | "document")))
                    && !blocks.iter().any(|b| b["type"] == "tool_result")
            });
        let starts_turn = matches!(body, Some(("user", _))) || attachment_prompt;
        self.records.insert(
            id.into(),
            Record {
                parent,
                body,
                starts_turn,
            },
        );
        if matches!(row["type"].as_str(), Some("user" | "assistant")) {
            self.leaf = Some(id.into());
        }
    }
    pub fn visible(&self) -> Vec<(&str, &'static str, &str)> {
        self.visible_with_turns()
            .into_iter()
            .map(|(id, role, text, _)| (id, role, text))
            .collect()
    }

    pub(super) fn visible_with_turns(&self) -> Vec<(&str, &'static str, &str, bool)> {
        let mut branch = Vec::new();
        let mut seen = HashSet::new();
        let mut next = self.leaf.as_deref();
        while let Some(id) = next {
            if !seen.insert(id) {
                break;
            }
            let Some(record) = self.records.get(id) else {
                break;
            };
            branch.push((id, record));
            next = record.parent.as_deref();
        }
        let mut start = false;
        branch
            .into_iter()
            .rev()
            .filter_map(|(id, record)| {
                start |= record.starts_turn;
                record
                    .body
                    .as_ref()
                    .map(|(role, text)| (id, *role, text.as_str(), std::mem::take(&mut start)))
            })
            .collect()
    }
}

fn visible_message(row: &Value, keep: impl Fn(&str) -> bool) -> Option<(&'static str, String)> {
    let role = match (row["type"].as_str()?, row["message"]["role"].as_str()?) {
        ("user", "user") => "user",
        ("assistant", "assistant") => "assistant",
        _ => return None,
    };
    if row["isMeta"] == true || row["isCompactSummary"] == true {
        return None;
    }
    let content = &row["message"]["content"];
    let visible =
        |text: &str| !text.trim_start().starts_with("<system-reminder>") && keep(text.trim_start());
    let text = if let Some(text) = content.as_str() {
        visible(text).then(|| text.to_owned())?
    } else {
        let blocks = content.as_array()?;
        if role == "user" && blocks.iter().any(|b| b["type"] == "tool_result") {
            return None;
        }
        blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .filter(|s| visible(s))
            .collect::<Vec<_>>()
            .join("\n")
    };
    (!text.trim().is_empty()).then_some((role, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn directory_title_excludes_tools_injected_context_and_other_sessions() {
        let rows = [
            json!({"type":"custom-title","sessionId":"other","customTitle":"其他会话"}),
            json!({"type":"custom-title","sessionId":"s","isSidechain":true,"customTitle":"子代理"}),
            json!({"type":"user","sessionId":"s","uuid":"meta","isMeta":true,"message":{"role":"user","content":"内部上下文"}}),
            json!({"type":"user","sessionId":"s","uuid":"compact","isCompactSummary":true,"message":{"role":"user","content":"上下文摘要"}}),
            json!({"type":"user","sessionId":"s","uuid":"tool","message":{"role":"user","content":[{"type":"tool_result","content":"输出"},{"type":"text","text":"内部注释"}]}}),
            json!({"type":"user","sessionId":"s","uuid":"command","message":{"role":"user","content":"<local-command-stdout>输出</local-command-stdout>"}}),
            json!({"type":"user","sessionId":"s","uuid":"q","message":{"role":"user","content":[{"type":"text","text":"<system-reminder>提示</system-reminder>"},{"type":"text","text":"  <button> 和 <a> 的区别？\n更多正文"}]}}),
        ];
        let mut title = SessionTitle::default();
        for row in rows {
            let line = format!("{row}\n");
            read_entries("s", line.as_bytes(), |r| title.insert(r)).unwrap();
        }
        assert_eq!(title.get(), "<button> 和 <a> 的区别？");
    }

    #[test]
    fn attachment_only_prompt_starts_a_new_readable_turn() {
        let mut transcript = Transcript::default();
        for row in [
            json!({"type":"user","uuid":"q","message":{"role":"user","content":"第一问"}}),
            json!({"type":"assistant","uuid":"a","parentUuid":"q","message":{"role":"assistant","content":"第一答"}}),
            json!({"type":"user","uuid":"image","parentUuid":"a","message":{"role":"user","content":[{"type":"image"}]}}),
            json!({"type":"assistant","uuid":"b","parentUuid":"image","message":{"role":"assistant","content":"图片说明"}}),
        ] {
            transcript.insert(&row);
        }
        assert_eq!(
            transcript.visible_with_turns(),
            vec![
                ("q", "user", "第一问", true),
                ("a", "assistant", "第一答", false),
                ("b", "assistant", "图片说明", true)
            ]
        );
    }

    #[test]
    fn reads_visible_main_branch_and_waits_for_complete_lines() {
        let rows = [
            json!({"type":"user","uuid":"q","sessionId":"s","message":{"role":"user","content":"问题"}}),
            json!({"type":"assistant","uuid":"old","parentUuid":"q","sessionId":"s","message":{"role":"assistant","content":[{"type":"text","text":"已舍弃的分支"}]}}),
            json!({"type":"assistant","uuid":"a","parentUuid":"q","sessionId":"s","message":{"role":"assistant","content":[{"type":"thinking","thinking":"内部"},{"type":"text","text":"正文"},{"type":"tool_use","input":{"text":"内部"}}]}}),
            json!({"type":"user","uuid":"tool","parentUuid":"a","sessionId":"s","message":{"role":"user","content":[{"type":"tool_result","content":"工具输出"}]}}),
            json!({"type":"assistant","uuid":"a2","parentUuid":"tool","sessionId":"s","message":{"role":"assistant","content":[{"type":"text","text":"完成"}]}}),
            json!({"type":"assistant","uuid":"sub","parentUuid":"a2","isSidechain":true,"sessionId":"s","message":{"role":"assistant","content":"私有"}}),
        ];
        let mut input = rows
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        input.push_str("\n{\"type\":");
        let mut transcript = Transcript::default();
        let consumed = transcript.append("s", input.as_bytes()).unwrap();
        assert_eq!(&input[consumed..], "{\"type\":");
        let body: Vec<_> = transcript
            .visible()
            .into_iter()
            .map(|(_, _, text)| text)
            .collect();
        assert_eq!(body, ["问题", "正文", "完成"]);
    }
}
