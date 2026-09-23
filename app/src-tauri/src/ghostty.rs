// Ghostty 标签页定位。Ghostty 1.3 起带 AppleScript 字典：terminal 有稳定 id、能 focus，
// 但没有 tty 属性（Terminal.app 靠的正是 tty），shell 环境里也没有 terminal id。
// 两边用探针对上：往会话的 tty 写一条 OSC 7（上报工作目录的转义序列），路径里带一次性记号，
// 再看哪个 terminal 的 working directory 变成了它；认出后立刻把原目录写回去。
// 不用标题做探针：Claude Code 会不停刷新标题，探针会被覆盖；OSC 7 它不写，屏幕上也不显示。
// terminal 存活期间它的 tty 不变，所以对应关系缓存起来，terminal 关掉后才需要重探。
// 见 docs/adr/0005-ghostty-osc7-probe.md

use crate::{dbg_log, osascript};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const BUNDLE_ID: &str = "com.mitchellh.ghostty";

struct Registry {
    // 宿主 tty -> terminal id
    ids: BTreeMap<String, String>,
    // 上一轮发现时的前台 terminal 与会话 tty：组合没变就不再探，
    // 否则普通 shell 标签页停在前台时每次心跳都会往会话 tty 里写探针
    last_scan: Option<String>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    ids: BTreeMap::new(),
    last_scan: None,
});

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

// 所有 terminal 的 (id, working directory)
fn terminals() -> Result<Vec<(String, String)>, String> {
    let out = osascript(
        r#"tell application "Ghostty"
    set ids to id of terminals
    set wds to working directory of terminals
end tell
set out to ""
repeat with i from 1 to count of ids
    set wd to item i of wds
    if wd is missing value then set wd to ""
    set out to out & item i of ids & tab & wd & linefeed
end repeat
return out"#,
    )?;
    Ok(out
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, wd)| (id.to_string(), wd.to_string()))
        .collect())
}

// Ghostty 只认主机名是本机或 localhost 的 OSC 7（空主机名会被忽略）；路径按 URL 编码
fn write_osc7(tty: &str, path: &str) -> std::io::Result<()> {
    let mut url = String::from("file://localhost");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    // O_NOCTTY：别让没有控制终端的 app 把会话的 tty 占成自己的；
    // O_NONBLOCK：输出被挂起（^S）的 tty 上写会一直阻塞
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(tty)?;
    f.write_all(format!("\x1b]7;{url}\x07").as_bytes())
}

// 一轮探多个 tty：各写一个带记号的目录，轮询到全部认出或超时，认出的写回原目录。
// 认不出的不写回——不知道记号落在了哪个 terminal 上
fn probe(reg: &mut Registry, ttys: &[String], before: &[(String, String)]) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let marks: Vec<(&String, String)> = ttys
        .iter()
        .enumerate()
        .filter_map(|(i, tty)| {
            let mark = format!("/red-green-probe/{stamp:x}-{i}");
            match write_osc7(tty, &mark) {
                Ok(()) => Some((tty, mark)),
                Err(e) => {
                    dbg_log(&format!("ghostty probe tty={tty} write_err={e}"));
                    None
                }
            }
        })
        .collect();
    let mut found: BTreeMap<&String, String> = BTreeMap::new();
    for _ in 0..6 {
        if let Ok(now) = terminals() {
            for (tty, mark) in &marks {
                if let Some((id, _)) = now.iter().find(|(_, wd)| wd == mark) {
                    found.insert(tty, id.clone());
                }
            }
        }
        if found.len() == marks.len() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for (tty, id) in &found {
        // 写回原目录：之后在这个 terminal 里开新标签页、分屏仍从原目录起步
        if let Some((_, wd)) = before.iter().find(|(i, wd)| i == id && !wd.is_empty()) {
            let _ = write_osc7(tty, wd);
        }
        reg.ids.insert(tty.to_string(), id.clone());
    }
    dbg_log(&format!("ghostty probe ttys={ttys:?} found={found:?}"));
}

fn tty_of(reg: &Registry, id: &str) -> Option<String> {
    reg.ids
        .iter()
        .find_map(|(tty, i)| (i == id).then(|| tty.clone()))
}

// focus 会选中标签页、把窗口提到最前，但 Ghostty 自己激活自己可能被 macOS 拦下
// （后台 app 抢前台有限制，实测偶尔没切过来）；补一个 activate，和 Terminal 那边一样
fn focus_id(id: &str) -> Result<String, String> {
    osascript(&format!(
        r#"tell application "Ghostty"
    focus terminal id "{id}"
    activate
end tell"#
    ))
}

// 点击宠物：切到 tty 所在的 terminal。
// 认不出就什么都不做——宁可不动，也不把错误的窗口带到前台。返回值只进现场日志
pub fn focus(tty: &str) -> String {
    let mut reg = registry();
    if let Some(id) = reg.ids.get(tty).cloned() {
        if focus_id(&id).is_ok() {
            return format!("ghostty=cached id={id}");
        }
        // terminal 已关掉，它的 tty 被新标签页复用了
        reg.ids.remove(tty);
    }
    let before = match terminals() {
        Ok(t) => t,
        Err(e) => return format!("ghostty_err={e}"),
    };
    probe(&mut reg, &[tty.to_string()], &before);
    match reg.ids.get(tty) {
        Some(id) => match focus_id(id) {
            Ok(_) => format!("ghostty=probed id={id}"),
            Err(e) => format!("ghostty=probed id={id} err={e}"),
        },
        None => "ghostty=not_found".to_string(),
    }
}

// 已阅检测与前台静默的心跳：前台 Ghostty terminal -> 宿主 tty。
// sessions 是各会话记录的 tty（按会话排好序）；ghostty_host 把其中跑在 Ghostty 里的
// 换成宿主 tty（tmux pane 换成挂着客户端的 tty），其余给 None
pub fn front_tty(
    sessions: Vec<String>,
    ghostty_host: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let front = osascript(
        r#"tell application "Ghostty" to return id of focused terminal of selected tab of front window"#,
    )
    .ok()?;
    let mut reg = registry();
    if let Some(tty) = tty_of(&reg, &front) {
        return Some(tty);
    }
    // 前台是没登记的 terminal：普通 shell 标签页、新开的会话、或复用了旧 tty 的新标签页
    let key = format!("{front} {sessions:?}");
    if reg.last_scan.as_deref() == Some(key.as_str()) {
        return None;
    }
    reg.last_scan = Some(key);
    let before = terminals().ok()?;
    // 关掉的 terminal 让出 tty，新标签页可能复用它：先清掉失效的对应关系
    reg.ids
        .retain(|_, id| before.iter().any(|(alive, _)| alive == id));
    let mut pending: Vec<String> = sessions
        .iter()
        .filter_map(|tty| ghostty_host(tty))
        .filter(|tty| !reg.ids.contains_key(tty))
        .collect();
    pending.sort();
    pending.dedup();
    if !pending.is_empty() {
        probe(&mut reg, &pending, &before);
    }
    tty_of(&reg, &front)
}
