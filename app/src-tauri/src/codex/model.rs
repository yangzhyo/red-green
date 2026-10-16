use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub fn bindings(
    rows: &[(String, String)],
    live_ttys: &HashSet<String>,
) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for (tty, title) in rows {
        if !live_ttys.contains(tty) {
            continue;
        }
        if let Some(id) = title_hint(title) {
            found
                .entry(id)
                .and_modify(|old: &mut String| {
                    if tty < old {
                        *old = tty.clone();
                    }
                })
                .or_insert_with(|| tty.clone());
        }
    }
    found
}

fn uuid_prefix(s: &str) -> bool {
    s.bytes().enumerate().all(|(i, b)| {
        if [8, 13, 18, 23].contains(&i) {
            b == b'-'
        } else {
            b.is_ascii_hexdigit()
        }
    })
}

// 0.162.1 原生标题将 thread-id 截成 29 字符 + "..."。
// 这里只收集提示；必须与所有已加载 ID 唯一匹配后才能绑定。
pub fn title_hint(title: &str) -> Option<String> {
    let mut ids = title
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '.')
        .filter_map(|s| {
            if s.len() == 36 && uuid_prefix(s) {
                Some(s)
            } else {
                s.strip_suffix("...")
                    .filter(|p| p.len() == 29 && uuid_prefix(p))
            }
        });
    let first = ids.next()?;
    if ids.any(|s| !s.eq_ignore_ascii_case(first)) {
        return None;
    }
    Some(first.to_ascii_lowercase())
}

pub fn resolve_id(hint: &str, loaded: &[String]) -> Option<String> {
    let mut matches = loaded
        .iter()
        .filter(|id| id.len() == 36 && uuid_prefix(id) && id.starts_with(hint));
    let first = matches.next()?;
    matches.next().is_none().then(|| first.clone())
}

fn is_question(text: &str) -> bool {
    let line = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let line: String = line
        .chars()
        .rev()
        .take(240)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let end = line.trim_end_matches(|c: char| c.is_whitespace() || "\"*_)）】`".contains(c));
    end.ends_with(['?', '？'])
        || [
            "请确认",
            "请选择",
            "请告诉我",
            "要不要",
            "你想",
            "你希望",
            "哪个",
            "哪种",
            "哪项",
            "哪一个",
            "哪一种",
            "哪一项",
        ]
        .iter()
        .any(|phrase| line.contains(phrase))
}

pub fn session(thread: &Value, turn: Option<&Value>, tty: &str, project: &str) -> Value {
    let Some(id) = thread["id"]
        .as_str()
        .filter(|id| id.len() == 36 && uuid_prefix(id))
    else {
        return Value::Null;
    };
    if !thread["parentThreadId"].is_null() || thread["source"].is_object() {
        return Value::Null;
    }
    let empty = Value::Null;
    let turn = turn.unwrap_or(&empty);
    let text = turn["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .rev()
                .find(|i| i["type"] == "agentMessage" && i["phase"] != "commentary")
        })
        .and_then(|i| i["text"].as_str())
        .unwrap_or("");
    let state = match thread["status"]["type"].as_str() {
        Some("active") => {
            if thread["status"]["activeFlags"]
                .as_array()
                .is_some_and(|flags| {
                    flags
                        .iter()
                        .any(|f| f == "waitingOnApproval" || f == "waitingOnUserInput")
                })
            {
                "awaiting"
            } else {
                "running"
            }
        }
        Some("systemError") => "aborted",
        Some("idle") => match turn["status"].as_str() {
            Some("failed") => "aborted",
            Some("completed") if is_question(text) => "your_turn",
            Some("completed") => "completed",
            _ => "idle",
        },
        _ => return Value::Null,
    };
    let detail = if state == "aborted" {
        turn["error"]["message"].as_str().unwrap_or(text)
    } else {
        text
    };
    let detail: String = if state == "your_turn" {
        detail
            .chars()
            .rev()
            .take(300)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    } else {
        detail.chars().take(300).collect()
    };
    json!({"session_id":format!("codex-{id}"), "source":"codex", "state":state,
        "cwd":thread["cwd"], "project":project, "tty":tty, "detail":detail,
        "since":format!("{}:{state}:{}",turn["id"].as_str().unwrap_or("new"),turn["completedAt"]),
        "event":"app-server"})
}

pub fn usage(raw: &Value, now: u64) -> Value {
    let account = raw["accountId"].as_str().unwrap_or("current");
    let fallback;
    let buckets = if let Some(map) = raw["rateLimitsByLimitId"].as_object() {
        map
    } else {
        fallback = serde_json::Map::from_iter([("codex".into(), raw["rateLimits"].clone())]);
        &fallback
    };
    let mut groups = Vec::new();
    for (id, bucket) in buckets {
        let mut windows = Vec::new();
        for key in ["primary", "secondary"] {
            let w = &bucket[key];
            let Some(pct) = w["usedPercent"]
                .as_f64()
                .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
            else {
                continue;
            };
            windows.push(json!({"key":key,"used_percentage":pct,
                "duration_minutes":w["windowDurationMins"].as_u64(), "resets_at":w["resetsAt"].as_u64()}));
        }
        if !windows.is_empty() {
            groups.push(json!({"id":format!("codex:{account}:{id}"), "source":"codex", "account_id":account,
                "label":if id == "codex" { "Codex" } else { bucket["limitName"].as_str().unwrap_or(id) },
                "windows":windows}));
        }
    }
    groups.sort_by_key(|g| {
        (
            !g["id"].as_str().unwrap_or("").ends_with(":codex"),
            g["id"].as_str().unwrap_or("").to_owned(),
        )
    });
    json!({"groups":groups,"updated_at":now})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "019a1234-1234-7123-8123-123456789abc";

    #[test]
    fn binds_only_a_complete_unambiguous_thread_id() {
        assert_eq!(
            title_hint(&format!("{ID} | Working | repo")),
            Some(ID.into())
        );
        assert_eq!(title_hint("repo | Working"), None);
        assert_eq!(title_hint(&format!("{ID}0")), None);
        assert_eq!(
            title_hint(&format!("{ID} 019a1234-1234-7123-8123-123456789abd")),
            None
        );
    }

    #[test]
    fn native_short_title_requires_one_loaded_id_and_rejects_collisions() {
        let short = format!("{}...", &ID[..29]);
        assert_eq!(title_hint(&short), Some(ID[..29].into()));
        assert_eq!(resolve_id(&ID[..29], &[ID.into()]), Some(ID.into()));
        let collision = "019a1234-1234-7123-8123-123456789abd";
        assert_eq!(resolve_id(&ID[..29], &[ID.into(), collision.into()]), None);
        assert_eq!(resolve_id(&ID[..29], &[]), None);
        assert_eq!(title_hint("019a1234..."), None);
    }

    #[test]
    fn bindings_drop_dead_processes_and_keep_same_directory_sessions_distinct() {
        let other = "019a1234-1234-7123-8123-123456789abd";
        let rows = vec![
            ("/dev/ttys001".into(), format!("{ID} | repo")),
            ("/dev/ttys002".into(), format!("{other} | repo")),
            ("/dev/ttys003".into(), format!("{ID} | stale")),
        ];
        let live = std::collections::HashSet::from(["/dev/ttys001".into(), "/dev/ttys002".into()]);
        let found = bindings(&rows, &live);
        assert_eq!(found.len(), 2);
        assert_eq!(found[ID], "/dev/ttys001");
        assert_eq!(found[other], "/dev/ttys002");
        assert!(bindings(&rows, &Default::default()).is_empty());
    }

    fn thread(status: serde_json::Value) -> serde_json::Value {
        json!({"id": ID, "cwd":"/tmp/project/subdir", "status":status, "source":"cli"})
    }

    #[test]
    fn active_waits_are_awaiting_and_do_not_use_old_completed_turn() {
        let old = json!({"id":"old", "status":"completed", "items":[]});
        for flag in ["waitingOnApproval", "waitingOnUserInput"] {
            let t = thread(json!({"type":"active", "activeFlags":[flag]}));
            assert_eq!(
                session(&t, Some(&old), "/dev/ttys001", "project")["state"],
                "awaiting"
            );
        }
        let t = thread(json!({"type":"active", "activeFlags":[]}));
        assert_eq!(
            session(&t, Some(&old), "/dev/ttys001", "project")["state"],
            "running"
        );
    }

    #[test]
    fn terminal_turn_status_and_final_line_drive_state() {
        let t = thread(json!({"type":"idle"}));
        for (status, text, want) in [
            ("completed", "正文有问题？\n已经完成。", "completed"),
            ("completed", "已经检查。\n要继续吗？**", "your_turn"),
            ("completed", "请确认下一步。", "your_turn"),
            ("failed", "", "aborted"),
            ("interrupted", "", "idle"),
        ] {
            let turn = json!({"id":"turn-1","status":status,"completedAt":12,"items":[{"type":"agentMessage","text":text}]});
            let s = session(&t, Some(&turn), "/dev/ttys001", "project");
            assert_eq!(s["state"], want, "{status}: {text}");
            assert_eq!(s["session_id"], format!("codex-{ID}"));
            assert_eq!(s["project"], "project");
            assert_eq!(s["tty"], "/dev/ttys001");
            assert_eq!(
                s["since"],
                session(&t, Some(&turn), "/dev/ttys001", "project")["since"]
            );
        }
    }

    #[test]
    fn new_session_idle_and_subagents_or_unloaded_are_absent() {
        assert_eq!(
            session(
                &thread(json!({"type":"idle"})),
                None,
                "/dev/ttys001",
                "project"
            )["state"],
            "idle"
        );
        assert!(session(
            &thread(json!({"type":"notLoaded"})),
            None,
            "/dev/ttys001",
            "project"
        )
        .is_null());
        let mut t = thread(json!({"type":"active","activeFlags":[]}));
        t["parentThreadId"] = json!(ID);
        assert!(session(&t, None, "/dev/ttys001", "project").is_null());
    }

    #[test]
    fn quota_keeps_account_and_buckets_with_actual_window_lengths() {
        let raw = json!({"accountId":"acct-a","rateLimits":{"primary":{"usedPercent":99}},"rateLimitsByLimitId":{
            "codex":{"primary":{"usedPercent":23,"windowDurationMins":300,"resetsAt":200},"secondary":{"usedPercent":41,"windowDurationMins":10080,"resetsAt":300}},
            "other":{"limitName":"Other","primary":{"usedPercent":7,"windowDurationMins":60,"resetsAt":210}}
        }});
        let u = usage(&raw, 100);
        assert_eq!(u["groups"].as_array().unwrap().len(), 2);
        assert_eq!(u["groups"][0]["account_id"], "acct-a");
        assert_eq!(
            u["groups"][0]["windows"][0]["used_percentage"].as_f64(),
            Some(23.0)
        );
        assert_eq!(u["groups"][1]["windows"][0]["duration_minutes"], 60);
        assert_eq!(u["updated_at"], 100);
    }

    #[test]
    fn missing_or_invalid_quota_stays_unknown() {
        let raw = json!({"rateLimits":{"primary":{"usedPercent":-2},"secondary":null}});
        assert!(usage(&raw, 100)["groups"].as_array().unwrap().is_empty());
        assert!(usage(&json!({}), 100)["groups"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
