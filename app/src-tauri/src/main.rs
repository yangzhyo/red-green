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

// 账号级用量文件：由 status line 脚本写入，与会话文件同目录、共用一个 watcher，
// 靠文件名区分（见 docs/protocol.md「用量文件」）
const USAGE_FILE: &str = "usage.json";

fn read_snapshot() -> Vec<Value> {
    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(status_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some(USAGE_FILE) {
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

// 文件不存在或不是合法 JSON 都返回 Null：前端据此整块不显示用量
fn read_usage() -> Value {
    std::fs::read_to_string(status_dir().join(USAGE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

#[tauri::command]
fn get_usage() -> Value {
    read_usage()
}

// ---- 宠物列的几何：沿工作区右缘从上往下排，用量表在最上，宠物从它下面依次往下叠 ----
// 屏幕右缘的上下两端都有常用按钮（聊天窗口右上的搜索、右下的发送），列从上排起、起点避开顶部工具栏，
// 下部留给发送按钮之类的东西
const PET_W: f64 = 116.0;
// 高度 = 内容 ~134 + 跳跃动画净空（振幅 18px）；再高只是死空间，会虚增视觉间距
const PET_H: f64 = 152.0;
// 内容在窗口内居中，精灵两侧留白约 22-27px，加上窗口边距视觉距右缘约 30px
const MARGIN_X: f64 = 4.0;
// 顶边距：工作区上缘（菜单栏之下）到用量表上缘。聊天类窗口贴顶时，工具栏（搜索等按钮）
// 约占工作区上缘以下 60–80pt，取 120 留出余量
const MARGIN_TOP: f64 = 120.0;
// 窗口间距 > 窗口高度：透明区重叠会抢走相邻宠物的点击
const SPACING: f64 = 158.0;
// 槽位间隙；用量表与 slot 0 之间也用它
const GAP: f64 = SPACING - PET_H;
// 用量表：与宠物同宽、同一条纵轴
const GAUGE_W: f64 = PET_W;
// 高度 = 像素圆表 60（15 格 × 4px）+ 顶部 2px + 底部投影 6px
const GAUGE_H: f64 = 68.0;
const GAUGE_LABEL: &str = "usage-gauge";
// slot 0 上缘距工作区上缘：给用量表留位，没有用量表时这块也空着，宠物位置不因它来去而变
const PET_TOP: f64 = MARGIN_TOP + GAUGE_H + GAP;

// 宠物列的锚点：主显示器工作区（不含 Dock 与菜单栏）的右上角，逻辑坐标；
// 显示器并排摆放时工作区原点不为零，所以要带上 position。取不到显示器时用固定点
fn column_anchor(app: &AppHandle) -> (f64, f64) {
    match app.primary_monitor() {
        Ok(Some(m)) => {
            let scale = m.scale_factor();
            let wa = m.work_area();
            let pos = wa.position.to_logical::<f64>(scale);
            let size = wa.size.to_logical::<f64>(scale);
            (pos.x + size.width, pos.y)
        }
        _ => (720.0, 0.0),
    }
}

// 宠物与用量表共用的窗口形态：透明、无边框、无阴影、置顶、跨所有桌面空间、
// 首次点击即生效（不用先激活窗口）
fn ambient_window(
    app: &AppHandle,
    label: &str,
    url: &str,
    title: &str,
    size: (f64, f64),
    pos: (f64, f64),
) -> Result<(), String> {
    tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(size.0, size.1)
        .position(pos.0, pos.1)
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
fn ensure_pet(app: AppHandle, sid: String, slot: u32) -> Result<(), String> {
    let label = format!("pet-{sid}");
    if app.get_webview_window(&label).is_some() {
        return Ok(());
    }
    // 宠物沿右缘从用量表下面往下叠
    let (right, top) = column_anchor(&app);
    let x = right - MARGIN_X - PET_W;
    let y = top + PET_TOP + slot as f64 * SPACING;
    ambient_window(
        &app,
        &label,
        &format!("pet.html?sid={sid}"),
        "red-green pet",
        (PET_W, PET_H),
        (x, y),
    )
}

// 用量表只有一个：列的最上面，下缘与 slot 0 宠物窗口隔一个槽位间隙
#[tauri::command]
fn ensure_gauge(app: AppHandle) -> Result<(), String> {
    if app.get_webview_window(GAUGE_LABEL).is_some() {
        return Ok(());
    }
    let (right, top) = column_anchor(&app);
    let x = right - MARGIN_X - GAUGE_W;
    let y = top + MARGIN_TOP;
    ambient_window(
        &app,
        GAUGE_LABEL,
        "gauge.html",
        "red-green usage gauge",
        (GAUGE_W, GAUGE_H),
        (x, y),
    )
}

#[tauri::command]
fn remove_gauge(app: AppHandle) {
    if let Some(w) = app.get_webview_window(GAUGE_LABEL) {
        let _ = w.close();
    }
}

// 光标在用量表窗口内时给出窗口内的逻辑坐标，否则 None。
// 两边先各自化为逻辑坐标再比较：cursor_position 按主显示器的缩放给物理坐标，
// outer_position / outer_size 按窗口所在显示器的缩放——用量表被拖到缩放不同的外接屏时两者基准不同
fn gauge_cursor_local(app: &AppHandle, w: &tauri::WebviewWindow) -> Option<Value> {
    let primary_scale = app.primary_monitor().ok()??.scale_factor();
    let c = app.cursor_position().ok()?.to_logical::<f64>(primary_scale);
    let scale = w.scale_factor().ok()?;
    let p = w.outer_position().ok()?.to_logical::<f64>(scale);
    let s = w.outer_size().ok()?.to_logical::<f64>(scale);
    let (dx, dy) = (c.x - p.x, c.y - p.y);
    if dx < 0.0 || dy < 0.0 || dx >= s.width || dy >= s.height {
        return None;
    }
    Some(serde_json::json!({ "x": dx, "y": dy }))
}

#[tauri::command]
fn remove_pet(app: AppHandle, sid: String) {
    if let Some(w) = app.get_webview_window(&format!("pet-{sid}")) {
        let _ = w.close();
    }
}

// 日志不能落在 status_dir：那是被 watch 的契约目录（协议规定 app 只读），
// 写进去每条日志都会自触发一次 sessions-changed
fn dbg_log(msg: &str) {
    if let Ok(home) = std::env::var("HOME") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{home}/Library/Logs/red-green.log"))
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

// tmux 客户端清单：(client_tty, client_session) 对
fn tmux_clients() -> Vec<(String, String)> {
    tmux(&["list-clients", "-F", "#{client_tty} #{client_session}"])
        .map(|out| {
            out.lines()
                .filter_map(|l| l.split_once(' ').map(|(t, s)| (t.to_string(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
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
    let clients = tmux_clients();
    let client = clients
        .iter()
        .find_map(|(ctty, csess)| (*csess == session).then(|| ctty.clone()));
    dbg_log(&format!(
        "tmux_locate pane={pane_tty} target={target} session={session} clients={clients:?} client={client:?}"
    ));
    Some((target, client))
}

// 前台 tab 挂着 tmux 客户端时，用户实际看到的是该 session 当前窗口的活动 pane
fn tmux_client_active_pane(client_tty: &str) -> Option<String> {
    let session = tmux_clients()
        .into_iter()
        .find_map(|(ctty, cs)| (ctty == client_tty).then_some(cs))?;
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
    dbg_log(&format!("focus_terminal {msg}"));
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

// 叫声文件名 = 皮肤-状态（如 crab-your_turn），产物由 scripts/gen-calls.mjs 生成、
// 随 bundle resources 打包；afplay 需要真实文件路径，所以不走前端资源而走 Resource 目录
#[tauri::command]
fn play_call(app: AppHandle, name: String) {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return;
    }
    let Ok(path) = app.path().resolve(
        format!("calls/{name}.wav"),
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
            get_usage,
            ensure_pet,
            remove_pet,
            ensure_gauge,
            remove_gauge,
            focus_terminal,
            frontmost_tty,
            play_call
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
                    // 用量文件与会话文件同目录：每次目录变化都顺带推一次，manager 侧去重
                    let _ = handle.emit("usage-changed", read_usage());
                }
            });

            // 用量表的悬停也在 Rust 侧：非焦点窗口收不到 WebKit 的 mousemove，
            // 只能轮询光标位置。进入窗口后持续推送窗口内的逻辑坐标（页面自己判定
            // 是否落在圆上），离开推一次 null；用量表不存在时什么都不做
            let hover = app.handle().clone();
            std::thread::spawn(move || {
                let mut inside = false;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    let Some(w) = hover.get_webview_window(GAUGE_LABEL) else {
                        inside = false;
                        continue;
                    };
                    let local = gauge_cursor_local(&hover, &w);
                    if local.is_some() || inside {
                        inside = local.is_some();
                        let _ = hover.emit_to(GAUGE_LABEL, "gauge-hover", local);
                    }
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
