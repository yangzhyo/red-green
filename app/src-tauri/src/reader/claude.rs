use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead};

struct Record {
    parent: Option<String>,
    body: Option<(&'static str, String)>,
}

#[derive(Default)]
pub struct Transcript {
    records: HashMap<String, Record>,
    leaf: Option<String>,
}

impl Transcript {
    /// Count complete lines so a growing file can resume before its unfinished tail.
    pub fn append(&mut self, session: &str, mut input: impl BufRead) -> io::Result<usize> {
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
                self.insert(session, &row);
            }
        }
        Ok(consumed)
    }
    fn insert(&mut self, session: &str, row: &Value) {
        if row["sessionId"].as_str() != Some(session) || row["isSidechain"] == true {
            return;
        }
        let Some(id) = row["uuid"].as_str() else {
            return;
        };
        let body = visible_message(row);
        let parent = row["parentUuid"].as_str().map(str::to_owned);
        self.records.insert(id.into(), Record { parent, body });
        if matches!(row["type"].as_str(), Some("user" | "assistant")) {
            self.leaf = Some(id.into());
        }
    }
    pub fn visible(&self) -> Vec<(&str, &'static str, &str)> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut next = self.leaf.as_deref();
        while let Some(id) = next {
            if !seen.insert(id) {
                break;
            }
            let Some(record) = self.records.get(id) else {
                break;
            };
            if let Some((role, text)) = &record.body {
                result.push((id, *role, text.as_str()));
            }
            next = record.parent.as_deref();
        }
        result.reverse();
        result
    }
}

fn visible_message(row: &Value) -> Option<(&'static str, String)> {
    let role = match (row["type"].as_str()?, row["message"]["role"].as_str()?) {
        ("user", "user") => "user",
        ("assistant", "assistant") => "assistant",
        _ => return None,
    };
    if row["isMeta"] == true || row["isCompactSummary"] == true {
        return None;
    }
    let content = &row["message"]["content"];
    let text = if let Some(text) = content.as_str() {
        text.to_owned()
    } else {
        let blocks = content.as_array()?;
        if role == "user" && blocks.iter().any(|b| b["type"] == "tool_result") {
            return None;
        }
        blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .filter(|s| !s.trim_start().starts_with("<system-reminder>"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    (!text.trim().is_empty() && !text.trim_start().starts_with("<system-reminder>"))
        .then_some((role, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
