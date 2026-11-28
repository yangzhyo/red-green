use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
struct LiveProc {
    tty: Option<String>,
    comm: String,
}

fn is_agy_comm(comm: &str) -> bool {
    let name = Path::new(comm.trim())
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    matches!(name, "agy" | "agy-cli")
}

fn parse_ps_output(out: &str) -> HashMap<u32, LiveProc> {
    let mut map = HashMap::new();
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((pid_str, after_pid)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        let Some((tty_raw, after_tty)) = after_pid.trim_start().split_once(char::is_whitespace)
        else {
            continue;
        };
        let comm_raw = after_tty.trim();
        if comm_raw.is_empty() {
            continue;
        }
        let tty = (tty_raw != "??" && !tty_raw.is_empty()).then(|| format!("/dev/{tty_raw}"));
        map.insert(
            pid,
            LiveProc {
                tty,
                comm: comm_raw.to_owned(),
            },
        );
    }
    map
}

fn query_live_processes(pids: &HashSet<u32>) -> HashMap<u32, LiveProc> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let mut list: Vec<u32> = pids.iter().copied().collect();
    list.sort_unstable();
    let arg = list
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let Ok(out) = Command::new("ps")
        .args(["-o", "pid=,tty=,comm=", "-p", &arg])
        .output()
    else {
        return HashMap::new();
    };
    parse_ps_output(&String::from_utf8_lossy(&out.stdout))
}

fn filter_with_map(sessions: Vec<Value>, procs: &HashMap<u32, LiveProc>) -> Vec<Value> {
    sessions
        .into_iter()
        .filter(|session| {
            if session["source"].as_str() != Some("antigravity") {
                return true;
            }
            let Some(pid) = session["pid"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .filter(|&p| p > 0)
            else {
                // 隔离验证注入的假会话文件不带 pid 时直接放行。
                return true;
            };
            let Some(proc) = procs.get(&pid) else {
                return false;
            };
            if !is_agy_comm(&proc.comm) {
                return false;
            }
            if let Some(tty) = session["tty"]
                .as_str()
                .filter(|t| t.starts_with("/dev/tty"))
            {
                return proc.tty.as_deref() == Some(tty);
            }
            true
        })
        .collect()
}

/// 过滤掉进程已退出（或 PID 已被其他进程复用）的 Antigravity CLI 会话。
pub fn filter_alive(sessions: Vec<Value>) -> Vec<Value> {
    if std::env::var_os("RED_GREEN_DISABLE_ANTIGRAVITY_PID_CHECK").is_some() {
        return sessions;
    }
    let pids: HashSet<u32> = sessions
        .iter()
        .filter(|s| s["source"].as_str() == Some("antigravity"))
        .filter_map(|s| s["pid"].as_u64().and_then(|v| u32::try_from(v).ok()))
        .filter(|&p| p > 0)
        .collect();
    if pids.is_empty() {
        return sessions;
    }
    let procs = query_live_processes(&pids);
    filter_with_map(sessions, &procs)
}

/// 校验指定 PID 是否仍是对应 tty 上存活的 Antigravity CLI 进程。
pub fn is_live_process(pid: u32, expected_tty: &str) -> bool {
    if std::env::var_os("RED_GREEN_DISABLE_ANTIGRAVITY_PID_CHECK").is_some() {
        return true;
    }
    if pid == 0 || !expected_tty.starts_with("/dev/tty") {
        return false;
    }
    let procs = query_live_processes(&HashSet::from([pid]));
    procs
        .get(&pid)
        .is_some_and(|p| is_agy_comm(&p.comm) && p.tty.as_deref() == Some(expected_tty))
}

/// 校验并归一化电纸书远程回复文本：统一换行，拒绝空串、超长或任何可能突破括号粘贴的控制字符。
pub fn validate_reply_text(text: &str) -> Result<String, u16> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.trim().is_empty() || normalized.chars().count() > 8192 {
        return Err(422);
    }
    if normalized
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(422);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_macos_ps_rows_including_multiple_spaces_and_paths_with_spaces() {
        let raw = "  2886   ttys000   /Users/y9g/.local/bin/agy\n  9999   ??   /Applications/My App/foo\n";
        let parsed = parse_ps_output(raw);
        assert_eq!(
            parsed.get(&2886),
            Some(&LiveProc {
                tty: Some("/dev/ttys000".into()),
                comm: "/Users/y9g/.local/bin/agy".into(),
            })
        );
        assert_eq!(
            parsed.get(&9999),
            Some(&LiveProc {
                tty: None,
                comm: "/Applications/My App/foo".into(),
            })
        );
    }

    #[test]
    fn filters_dead_reused_and_mismatched_tty_while_keeping_injected_and_other_sources() {
        let procs = HashMap::from([
            (
                100,
                LiveProc {
                    tty: Some("/dev/ttys001".into()),
                    comm: "agy".into(),
                },
            ),
            (
                200,
                LiveProc {
                    tty: Some("/dev/ttys002".into()),
                    comm: "/bin/zsh".into(),
                },
            ),
            (
                300,
                LiveProc {
                    tty: Some("/dev/ttys009".into()),
                    comm: "/Users/y9g/.local/bin/agy".into(),
                },
            ),
        ]);
        let sessions = vec![
            json!({"session_id":"c1","state":"running"}),
            json!({"session_id":"vfy","source":"antigravity","state":"awaiting","tty":"/dev/ttys099"}),
            json!({"session_id":"pid-100","source":"antigravity","pid":100,"tty":"/dev/ttys001"}),
            json!({"session_id":"pid-200","source":"antigravity","pid":200,"tty":"/dev/ttys002"}),
            json!({"session_id":"pid-300","source":"antigravity","pid":300,"tty":"/dev/ttys003"}),
            json!({"session_id":"pid-404","source":"antigravity","pid":404,"tty":"/dev/ttys004"}),
        ];
        let kept: Vec<_> = filter_with_map(sessions, &procs)
            .into_iter()
            .map(|s| s["session_id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(kept, ["c1", "vfy", "pid-100"]);
    }

    #[test]
    fn validates_reply_text_and_rejects_terminal_control_characters() {
        assert_eq!(
            validate_reply_text("第一行\r\n第二行\t继续").as_deref(),
            Ok("第一行\n第二行\t继续")
        );
        assert_eq!(validate_reply_text("   \n\t "), Err(422));
        assert_eq!(validate_reply_text("hi\x1b[201~bad"), Err(422));
        assert_eq!(validate_reply_text("hi\x03"), Err(422));
        assert_eq!(validate_reply_text("hi\x04"), Err(422));
        assert_eq!(validate_reply_text(&"a".repeat(8193)), Err(422));
    }
}
