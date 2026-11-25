use serde_json::Value;

/// Preserve the order of items inside one full App Server turn.
pub fn visible_items(turn: &Value) -> Vec<(String, &'static str, String)> {
    let Some(turn_id) = turn["id"].as_str() else {
        return Vec::new();
    };
    turn["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item["id"].as_str()?;
            let (role, text) = match item["type"].as_str()? {
                "userMessage" => {
                    let text = item["content"]
                        .as_array()?
                        .iter()
                        .filter(|part| part["type"] == "text")
                        .filter_map(|part| part["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ("user", text)
                }
                "agentMessage" => ("assistant", item["text"].as_str()?.to_owned()),
                _ => return None,
            };
            (!text.trim().is_empty()).then(|| (format!("{turn_id}/{id}"), role, text))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn projects_only_reader_visible_text_and_uses_ids_scoped_to_the_turn() {
        let turn = json!({"id":"turn","items":[
            {"type":"userMessage","id":"u","content":[{"type":"text","text":"问题"},{"type":"localImage","path":"/private"}]},
            {"type":"commandExecution","id":"tool","aggregatedOutput":"工具输出"},
            {"type":"reasoning","id":"r","summary":["内部"]},
            {"type":"agentMessage","id":"a","text":"答复"}
        ]});
        assert_eq!(
            visible_items(&turn),
            vec![
                ("turn/u".into(), "user", "问题".into()),
                ("turn/a".into(), "assistant", "答复".into())
            ]
        );
    }
}
