use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, BufRead};

/// 只消费以换行符结尾的完整 JSON 行，避免读取正在追加写入的半行。
pub(super) fn read_entries(
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
            insert(&row);
        }
    }
    Ok(consumed)
}

fn strip_tag_block(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = text.to_owned();
    while let Some(start) = out.find(&open) {
        if let Some(end_rel) = out[start..].find(&close) {
            let end = start + end_rel + close.len();
            out.replace_range(start..end, "");
        } else {
            out.truncate(start);
            break;
        }
    }
    out
}

/// 剥离 Antigravity CLI 在 USER_INPUT 中附加的系统元数据包装，仅保留面向读者的用户提问。
pub fn extract_user_text(raw: &str) -> String {
    if let Some(start) = raw.find("<USER_REQUEST>") {
        let after = &raw[start + "<USER_REQUEST>".len()..];
        let body = match after.find("</USER_REQUEST>") {
            Some(end) => &after[..end],
            None => after,
        };
        return body.trim().to_owned();
    }
    let mut cleaned = raw.to_owned();
    for tag in [
        "ADDITIONAL_METADATA",
        "USER_SETTINGS_CHANGE",
        "SYSTEM_MESSAGE",
    ] {
        cleaned = strip_tag_block(&cleaned, tag);
    }
    cleaned.trim().to_owned()
}

fn visible_message(row: &Value) -> Option<(&'static str, String)> {
    match (row["type"].as_str()?, row["source"].as_str()?) {
        ("USER_INPUT", "USER_EXPLICIT") => {
            let text = extract_user_text(row["content"].as_str()?);
            (!text.is_empty()).then_some(("user", text))
        }
        ("PLANNER_RESPONSE", "MODEL") => {
            let text = row["content"].as_str()?.trim();
            (!text.is_empty()).then(|| ("assistant", text.to_owned()))
        }
        _ => None,
    }
}

#[derive(Default)]
pub(super) struct SessionTitle {
    first_prompt: Option<String>,
}

impl SessionTitle {
    pub(super) fn get(&self) -> &str {
        self.first_prompt.as_deref().unwrap_or("未命名会话")
    }

    pub(super) fn insert(&mut self, row: &Value) {
        if self.first_prompt.is_some() {
            return;
        }
        if let Some(("user", text)) = visible_message(row) {
            self.first_prompt = text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| line.chars().take(120).collect());
        }
    }
}

struct Record {
    id: String,
    body: Option<(&'static str, String)>,
    starts_turn: bool,
}

#[derive(Default)]
pub struct Transcript {
    records: BTreeMap<i64, Record>,
    next_fallback: i64,
}

impl Transcript {
    pub fn append(&mut self, input: impl BufRead) -> io::Result<usize> {
        read_entries(input, |row| self.insert(row))
    }

    pub(super) fn insert(&mut self, row: &Value) {
        let is_explicit_user = row["type"] == "USER_INPUT" && row["source"] == "USER_EXPLICIT";
        let body = visible_message(row);
        if body.is_none() && !is_explicit_user {
            return;
        }
        let step = row["step_index"].as_i64().unwrap_or_else(|| {
            let v = self.next_fallback;
            self.next_fallback += 1;
            v
        });
        if step >= self.next_fallback {
            self.next_fallback = step + 1;
        }
        self.records.insert(
            step,
            Record {
                id: format!("step-{step}"),
                body,
                starts_turn: is_explicit_user,
            },
        );
    }

    pub fn visible(&self) -> Vec<(&str, &'static str, &str)> {
        self.visible_with_turns()
            .into_iter()
            .map(|(id, role, text, _)| (id, role, text))
            .collect()
    }

    pub(super) fn visible_with_turns(&self) -> Vec<(&str, &'static str, &str, bool)> {
        let mut start = false;
        self.records
            .values()
            .filter_map(|record| {
                start |= record.starts_turn;
                record.body.as_ref().map(|(role, text)| {
                    (
                        record.id.as_str(),
                        *role,
                        text.as_str(),
                        std::mem::take(&mut start),
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_user_request_and_skips_thinking_tools_and_generic_outputs() {
        let rows = [
            json!({
                "step_index": 0,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "content": "<USER_REQUEST>\n准备动手做 Issue#42\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nThe current local time is: 2026-11-28\n</ADDITIONAL_METADATA>\n<USER_SETTINGS_CHANGE>\nchanged\n</USER_SETTINGS_CHANGE>"
            }),
            json!({
                "step_index": 1,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "thinking": "内部思考不应出现在阅读页",
                "tool_calls": [{"name": "run_command", "args": {"CommandLine": "ls"}}]
            }),
            json!({
                "step_index": 2,
                "source": "MODEL",
                "type": "GENERIC",
                "content": "Command exited with code 0.\nsecret output"
            }),
            json!({
                "step_index": 3,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "thinking": "整理结论",
                "content": "已经检查完毕，是否继续？"
            }),
        ];
        let mut input = rows
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        input.push_str("\n{\"step_index\":4,\"partial\":");

        let mut transcript = Transcript::default();
        let consumed = transcript.append(input.as_bytes()).unwrap();
        assert_eq!(&input[consumed..], "{\"step_index\":4,\"partial\":");
        assert_eq!(
            transcript.visible_with_turns(),
            vec![
                ("step-0", "user", "准备动手做 Issue#42", true),
                ("step-3", "assistant", "已经检查完毕，是否继续？", false),
            ]
        );
    }

    #[test]
    fn empty_user_prompt_still_starts_a_new_turn_and_title_takes_first_line() {
        let rows = [
            json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>\n  第一行标题  \n第二行详情\n</USER_REQUEST>"}),
            json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","content":"第一答"}),
            json!({"step_index":2,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>\n\n</USER_REQUEST>"}),
            json!({"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","content":"附件说明"}),
        ];
        let mut title = SessionTitle::default();
        let mut transcript = Transcript::default();
        for row in &rows {
            title.insert(row);
            transcript.insert(row);
        }
        assert_eq!(title.get(), "第一行标题");
        assert_eq!(
            transcript.visible_with_turns(),
            vec![
                ("step-0", "user", "第一行标题  \n第二行详情", true),
                ("step-1", "assistant", "第一答", false),
                ("step-3", "assistant", "附件说明", true),
            ]
        );
    }
}
