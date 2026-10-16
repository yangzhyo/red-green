//! 客户端自己的标题绑定 thread → tty；不使用后台服务继承的终端环境变量。
use crate::{ghostty, osascript, tmux, tty_host, Host};
use red_green::codex::model;
use std::collections::{BTreeMap, HashSet};
use std::process::Command;

pub fn discover() -> BTreeMap<String, String> {
    let output = match Command::new("ps").args(["-axo", "tty=,comm="]).output() {
        Ok(o) if o.status.success() => o,
        _ => return BTreeMap::new(),
    };
    let live: HashSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (tty, command) = line.trim().split_once(char::is_whitespace)?;
            (tty != "??" && std::path::Path::new(command.trim()).file_name()?.to_str()? == "codex")
                .then(|| format!("/dev/{tty}"))
        })
        .collect();
    if live.is_empty() {
        return BTreeMap::new();
    }
    let mut rows = Vec::new();
    // tmux pane 标题独立于宿主 tab，先按 pane 的准确 tty 收集。
    if let Some(panes) = tmux(&["list-panes", "-a", "-F", "#{pane_tty}\t#{pane_title}"]) {
        rows.extend(parse_rows(&panes));
    }
    // 先判断是否运行，避免 AppleScript 为观察动作启动终端。
    if running("Terminal") {
        if let Ok(out) = osascript(
            r#"tell application "Terminal"
    set output to ""
    repeat with w in windows
        repeat with t in tabs of w
            set output to output & (tty of t) & (ASCII character 9) & (custom title of t) & linefeed
        end repeat
    end repeat
    return output
end tell"#,
        ) {
            rows.extend(parse_rows(&out));
        }
    }
    if running("ghostty") || running("Ghostty") {
        let candidates: Vec<String> = live
            .iter()
            .filter(|tty| tty_host(tty) == Some(Host::Ghostty))
            .cloned()
            .collect();
        if !candidates.is_empty() {
            if let Ok(out) = osascript(
                r#"tell application "Ghostty"
    set output to ""
    repeat with t in terminals
        set output to output & (id of t) & (ASCII character 9) & (name of t) & linefeed
    end repeat
    return output
end tell"#,
            ) {
                let titles: Vec<_> = parse_rows(&out)
                    .into_iter()
                    .filter(|(_, title)| model::title_hint(title).is_some())
                    .collect();
                if !titles.is_empty() {
                    let mapped = ghostty::map_ttys(&candidates);
                    for (id, title) in titles {
                        if let Some(tty) = mapped.get(&id) {
                            rows.push((tty.clone(), title));
                        }
                    }
                }
            }
        }
    }
    model::bindings(&rows, &live)
}

fn parse_rows(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
}

fn running(name: &str) -> bool {
    Command::new("pgrep")
        .args(["-x", name])
        .output()
        .is_ok_and(|o| o.status.success())
}
