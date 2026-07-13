#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use tauri::{AppHandle, Emitter, Manager};

fn status_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME not set"))
        .join(".claude")
        .join("session-status")
}

fn read_snapshot() -> Vec<Value> {
    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(status_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    sessions.push(v);
                }
            }
        }
    }
    sessions.sort_by(|a, b| {
        a["session_id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["session_id"].as_str().unwrap_or(""))
    });
    sessions
}

#[tauri::command]
fn get_sessions() -> Vec<Value> {
    read_snapshot()
}

#[tauri::command]
fn ensure_pet(app: AppHandle, sid: String, slot: u32) -> Result<(), String> {
    let label = format!("pet-{sid}");
    if app.get_webview_window(&label).is_some() {
        return Ok(());
    }
    const PET_W: f64 = 116.0;
    // 高度 = 内容 ~134 + 跳跃动画净空（振幅 18px）；再高只是死空间，会虚增视觉间距
    const PET_H: f64 = 152.0;
    // 内容在窗口内居中，精灵两侧留白约 22-27px，加上窗口边距视觉距右缘约 30px
    const MARGIN_X: f64 = 4.0;
    const MARGIN_Y: f64 = 132.0;
    // 窗口间距 > 窗口高度：透明区重叠会抢走相邻宠物的点击
    const SPACING: f64 = 158.0;

    // pets stack vertically along the right edge of the primary monitor's
    // work area (excludes the Dock and menu bar), growing bottom-up from
    // the bottom-right corner; monitor origin matters when displays are
    // arranged side by side
    let (x, y) = match app.primary_monitor() {
        Ok(Some(m)) => {
            let scale = m.scale_factor();
            let wa = m.work_area();
            let pos = wa.position.to_logical::<f64>(scale);
            let size = wa.size.to_logical::<f64>(scale);
            (
                pos.x + size.width - MARGIN_X - PET_W,
                pos.y + size.height - PET_H - MARGIN_Y - slot as f64 * SPACING,
            )
        }
        _ => (600.0, 600.0 - slot as f64 * SPACING),
    };

    tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::App(format!("pet.html?sid={sid}").into()),
    )
    .title("red-green pet")
    .inner_size(PET_W, PET_H)
    .position(x, y)
    .transparent(true)
    .decorations(false)
    .shadow(false)
    .resizable(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .accept_first_mouse(true)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn remove_pet(app: AppHandle, sid: String) {
    if let Some(w) = app.get_webview_window(&format!("pet-{sid}")) {
        let _ = w.close();
    }
}

fn dbg_log(msg: &str) {
    if let Ok(home) = std::env::var("HOME") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{home}/.claude/session-status/.app-debug.log"))
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}

// tmux 常见安装路径逐个尝试：将来以 .app 方式从 launchd 启动时没有 brew PATH
fn tmux(args: &[&str]) -> Option<String> {
    for bin in ["tmux", "/opt/homebrew/bin/tmux", "/usr/local/bin/tmux"] {
        match Command::new(bin).args(args).output() {
            Ok(out) if out.status.success() => {
                return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
            }
            // 失败不记日志：心跳每 1.5s 走这里，无 tmux 的环境会刷爆日志文件
            Ok(_) => return None, // tmux 存在但命令失败（如无 server）
            Err(_) => continue,
        }
    }
    None
}

// pane tty -> (tmux 目标 "session:window.pane", 挂载客户端的 tty)
fn tmux_locate(pane_tty: &str) -> Option<(String, Option<String>)> {
    // 分隔符用空格：tmux 会把输出中的控制字符（含制表符）消毒成 "_"
    let panes = tmux(&[
        "list-panes",
        "-a",
        "-F",
        "#{pane_tty} #{session_name}:#{window_index}.#{pane_index}",
    ])?;
    let target = match panes.lines().find_map(|l| {
        let (ptty, tgt) = l.split_once(' ')?;
        (ptty == pane_tty).then(|| tgt.to_string())
    }) {
        Some(t) => t,
        None => {
            dbg_log(&format!(
                "tmux_locate pane={pane_tty} not found, raw panes={panes:?}"
            ));
            return None;
        }
    };
    let session = target.split(':').next().unwrap_or_default().to_string();
    let clients_raw = tmux(&["list-clients", "-F", "#{client_tty} #{client_session}"]);
    let client = clients_raw.as_deref().and_then(|s| {
        s.lines().find_map(|l| {
            let (ctty, csess) = l.split_once(' ')?;
            (csess == session).then(|| ctty.to_string())
        })
    });
    dbg_log(&format!(
        "tmux_locate pane={pane_tty} target={target} session={session} clients={clients_raw:?} client={client:?}"
    ));
    Some((target, client))
}

// 前台 tab 挂着 tmux 客户端时，用户实际看到的是该 session 当前窗口的活动 pane
fn tmux_client_active_pane(client_tty: &str) -> Option<String> {
    let clients = tmux(&["list-clients", "-F", "#{client_tty} #{client_session}"])?;
    let session = clients.lines().find_map(|l| {
        let (ctty, cs) = l.split_once(' ')?;
        (ctty == client_tty).then(|| cs.to_string())
    })?;
    tmux(&["display-message", "-p", "-t", &session, "#{pane_tty}"])
}

#[tauri::command]
fn focus_terminal(tty: String) {
    if tty.is_empty() || !tty.starts_with("/dev/tty") {
        return;
    }
    // tmux pane 里的会话：先让 tmux 切到对应 session/window/pane，
    // 再把「找 tab」的目标换成挂着 tmux 客户端的真实 tty
    let mut tab_tty = tty.clone();
    if let Some((target, client_tty)) = tmux_locate(&tty) {
        let session = target.split(':').next().unwrap_or_default().to_string();
        let window = target
            .rsplit_once('.')
            .map(|(w, _)| w.to_string())
            .unwrap_or_else(|| target.clone());
        let _ = tmux(&["switch-client", "-t", &session]);
        let _ = tmux(&["select-window", "-t", &window]);
        let _ = tmux(&["select-pane", "-t", &target]);
        if let Some(ct) = client_tty {
            tab_tty = ct;
        }
    }
    // set frontmost 比 set index 可靠；activate 必须在命中之后——
    // 放在最前会在找不到 tab 时把 Terminal 连同错误的窗口带到前台
    let script = format!(
        r#"tell application "Terminal"
    repeat with w in windows
        repeat with t in tabs of w
            if tty of t is "{tab_tty}" then
                set selected of t to true
                set frontmost of w to true
                activate
                return
            end if
        end repeat
    end repeat
end tell"#
    );
    let out = Command::new("osascript").arg("-e").arg(script).output();
    // 现场日志：点击定位涉及 tmux 解析 + TCC 权限 + AppleScript 三层，
    // 出问题时凭这行就能定位是哪层
    let msg = match &out {
        Ok(o) => format!(
            "tty={} tab_tty={} osascript_ok={} err={}",
            tty,
            tab_tty,
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => format!("tty={} tab_tty={} spawn_err={}", tty, tab_tty, e),
    };
    if let Ok(home) = std::env::var("HOME") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{home}/.claude/session-status/.app-debug.log"))
        {
            let _ = writeln!(f, "focus_terminal {msg}");
        }
    }
}

fn frontmost_tty_impl() -> Option<String> {
    let script = r#"tell application "System Events"
    set frontApp to bundle identifier of first process whose frontmost is true
end tell
if frontApp is "com.apple.Terminal" then
    tell application "Terminal" to return tty of selected tab of front window
else
    return ""
end if"#;
    let out = Command::new("osascript").arg("-e").arg(script).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        return None;
    }
    if let Some(pane) = tmux_client_active_pane(&s) {
        return Some(pane);
    }
    Some(s)
}

#[tauri::command]
fn frontmost_tty() -> Option<String> {
    frontmost_tty_impl()
}

// 叫声文件名 = 皮肤-状态（如 crab-your_turn），产物由 scripts/gen-sounds.mjs 生成、
// 随 bundle resources 打包；afplay 需要真实文件路径，所以不走前端资源而走 Resource 目录
#[tauri::command]
fn play_sound(app: AppHandle, name: String) {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return;
    }
    let Ok(path) = app.path().resolve(
        format!("sounds/{name}.wav"),
        tauri::path::BaseDirectory::Resource,
    ) else {
        return;
    };
    let _ = Command::new("afplay").arg(path).spawn();
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_sessions,
            ensure_pet,
            remove_pet,
            focus_terminal,
            frontmost_tty,
            play_sound
        ])
        .setup(|app| {
            // pets are ambient: no dock icon, no app switcher entry
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let dir = status_dir();
            std::fs::create_dir_all(&dir).ok();

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                use notify::{RecursiveMode, Watcher};
                let (tx, rx) = std::sync::mpsc::channel();
                let mut watcher = match notify::recommended_watcher(tx) {
                    Ok(w) => w,
                    Err(_) => return,
                };
                if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
                    return;
                }
                while rx.recv().is_ok() {
                    // swallow the burst, then push one snapshot
                    while rx
                        .recv_timeout(std::time::Duration::from_millis(120))
                        .is_ok()
                    {}
                    let _ = handle.emit("sessions-changed", read_snapshot());
                }
            });

            // 已阅心跳必须在 Rust 侧：manager 是隐藏窗口，
            // WebKit 会挂起不可见 webview 的 JS 定时器（事件不受影响）
            let heartbeat = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let _ = heartbeat.emit("front-tick", frontmost_tty_impl());
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running red-green");
}
