// Ghostty 标签页定位：Ghostty 的 AppleScript 里 terminal 有 id、能 focus，但没有 tty 属性。
// 往会话的 tty 写一条带记号的 OSC 7，看哪个 terminal 的工作目录变成了记号，由此把 tty 对上 terminal id 并缓存。
// 取舍与代价见 docs/adr/0005-ghostty-osc7-probe.md

use crate::{dbg_log, osascript};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// 探针没认全时，同样的现状隔这么久才重探：认不出的记号写不回去，不能每次心跳都往里写
const RETRY_AFTER: Duration = Duration::from_secs(30);

struct Registry {
    id_by_tty: BTreeMap<String, String>,
    // 普通 shell 标签页停在前台时，心跳每 1.5s 都会碰到没登记的 terminal；
    // 现状没变就不再探，否则每次心跳都要往会话的 tty 里写探针
    last_scan: Option<Scan>,
}

struct Scan {
    // 前台 terminal + 会话与 tmux 客户端的现状
    situation: String,
    // None：上一轮全认出了，现状不变就不必再探
    retry_at: Option<Instant>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    id_by_tty: BTreeMap::new(),
    last_scan: None,
});

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

struct Surface {
    id: String,
    working_dir: String,
}

fn surfaces() -> Result<Vec<Surface>, String> {
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
        .map(|(id, wd)| Surface {
            id: id.to_string(),
            working_dir: wd.to_string(),
        })
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
fn probe(reg: &mut Registry, ttys: &[String], before: &[Surface]) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let marks: Vec<(&String, String)> = ttys
        .iter()
        .enumerate()
        .filter_map(|(n, tty)| {
            let mark = format!("/red-green-probe/{stamp:x}-{n}");
            match write_osc7(tty, &mark) {
                Ok(()) => Some((tty, mark)),
                Err(e) => {
                    dbg_log(&format!("ghostty probe tty={tty} write_err={e}"));
                    None
                }
            }
        })
        .collect();
    // 实测写入后下一次读就能看到新目录；留约 300ms 给正忙着的 Ghostty
    let mut found: BTreeMap<&String, String> = BTreeMap::new();
    for _ in 0..6 {
        if let Ok(now) = surfaces() {
            for (tty, mark) in &marks {
                if let Some(s) = now.iter().find(|s| s.working_dir == *mark) {
                    found.insert(tty, s.id.clone());
                }
            }
        }
        if found.len() == marks.len() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for (tty, id) in &found {
        // 写回原目录：之后从这个 terminal 开新标签页、分屏仍从原目录起步。
        // 原来没上报过目录的（没开 shell integration）写主目录，免得记号路径一直留在上面
        let restore = before
            .iter()
            .find(|s| s.id == *id && !s.working_dir.is_empty())
            .map(|s| s.working_dir.clone())
            .or_else(|| std::env::var("HOME").ok());
        if let Some(dir) = restore {
            let _ = write_osc7(tty, &dir);
        }
        reg.id_by_tty.insert(tty.to_string(), id.clone());
    }
    dbg_log(&format!("ghostty probe ttys={ttys:?} found={found:?}"));
}

fn tty_of(reg: &Registry, id: &str) -> Option<String> {
    reg.id_by_tty
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
    if let Some(id) = reg.id_by_tty.get(tty).cloned() {
        if focus_id(&id).is_ok() {
            return format!("ghostty=cached id={id}");
        }
        // terminal 已关掉，它的 tty 被新标签页复用了
        reg.id_by_tty.remove(tty);
    }
    let before = match surfaces() {
        Ok(s) => s,
        Err(e) => return format!("ghostty_err={e}"),
    };
    probe(&mut reg, &[tty.to_string()], &before);
    match reg.id_by_tty.get(tty) {
        Some(id) => match focus_id(id) {
            Ok(_) => format!("ghostty=probed id={id}"),
            Err(e) => format!("ghostty=probed id={id} err={e}"),
        },
        None => "ghostty=not_found".to_string(),
    }
}

// 已阅检测与前台静默的心跳：前台 Ghostty terminal -> 标签页 tty。
// 前台是没登记的 terminal（普通 shell 标签页、新开的会话、刚 attach 的 tmux 客户端、复用了旧 tty 的新标签页）
// 时发现一轮：situation() 描述会话与 tmux 客户端的现状，连同前台 terminal 都没变就不重探；
// tab_ttys() 给出跑着会话的 Ghostty 标签页 tty，开销大，只在要探时调用
pub fn front_tty(
    situation: impl FnOnce() -> String,
    tab_ttys: impl FnOnce() -> Vec<String>,
) -> Option<String> {
    let front = osascript(
        r#"tell application "Ghostty" to return id of focused terminal of selected tab of front window"#,
    )
    .ok()?;
    let mut reg = registry();
    if let Some(tty) = tty_of(&reg, &front) {
        return Some(tty);
    }
    let situation = format!("{front} {}", situation());
    if let Some(scan) = &reg.last_scan {
        if scan.situation == situation && scan.retry_at.is_none_or(|t| Instant::now() < t) {
            return None;
        }
    }
    let before = surfaces().ok()?;
    // 关掉的 terminal 让出 tty，新标签页可能复用它：先清掉失效的对应关系
    reg.id_by_tty
        .retain(|_, id| before.iter().any(|s| s.id == *id));
    let mut pending: Vec<String> = tab_ttys()
        .into_iter()
        .filter(|tty| !reg.id_by_tty.contains_key(tty))
        .collect();
    pending.sort();
    pending.dedup();
    if !pending.is_empty() {
        probe(&mut reg, &pending, &before);
    }
    let missed = pending.iter().any(|tty| !reg.id_by_tty.contains_key(tty));
    reg.last_scan = Some(Scan {
        situation,
        retry_at: missed.then(|| Instant::now() + RETRY_AFTER),
    });
    tty_of(&reg, &front)
}
