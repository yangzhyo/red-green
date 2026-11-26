use red_green::reader::{live::LiveSource, Reader};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    process::Command,
    sync::{LazyLock, Mutex},
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    App, Listener,
};

pub(super) type Acknowledged = HashMap<String, serde_json::Value>;
static ACKNOWLEDGED: LazyLock<Mutex<Acknowledged>> = LazyLock::new(Mutex::default);

pub fn update_acknowledged(acknowledged: Acknowledged) {
    *ACKNOWLEDGED.lock().unwrap_or_else(|e| e.into_inner()) = acknowledged;
}

fn reader_snapshot() -> Vec<serde_json::Value> {
    let mut sessions = super::read_snapshot();
    let acknowledged = ACKNOWLEDGED.lock().unwrap_or_else(|e| e.into_inner());
    apply_acknowledged(&mut sessions, &acknowledged);
    sessions
}

fn apply_acknowledged(sessions: &mut [serde_json::Value], acknowledged: &Acknowledged) {
    for session in sessions {
        if !matches!(session["state"].as_str(), Some("completed" | "aborted")) {
            continue;
        }
        let Some(sid) = session["session_id"].as_str() else {
            continue;
        };
        let source = session["source"].as_str().unwrap_or("claude");
        if acknowledged
            .get(&format!("{source}-{sid}"))
            .is_some_and(|since| since == &session["since"])
        {
            session["state"] = "idle".into();
        }
    }
}

fn session_summary(sessions: &[serde_json::Value]) -> String {
    if sessions.is_empty() {
        return "当前没有会话".into();
    }
    let pending = sessions.iter().filter(|s| s["state"] == "awaiting").count();
    let mut text = format!("当前 {} 个会话", sessions.len());
    if pending > 0 {
        text.push_str(&format!(" · {pending} 个待确认"));
    }
    text
}

fn wifi_address() -> Result<SocketAddr, String> {
    let ports = Command::new("/usr/sbin/networksetup")
        .arg("-listallhardwareports")
        .output()
        .map_err(|e| e.to_string())?;
    let ports = String::from_utf8_lossy(&ports.stdout);
    let interface = ports
        .split("\n\n")
        .find(|p| {
            p.lines()
                .any(|s| s == "Hardware Port: Wi-Fi" || s == "Hardware Port: AirPort")
        })
        .and_then(|p| p.lines().find_map(|s| s.strip_prefix("Device: ")))
        .ok_or("找不到 Wi-Fi 接口")?;
    let config = Command::new("/sbin/ifconfig")
        .arg(interface)
        .output()
        .map_err(|e| e.to_string())?;
    let ip = String::from_utf8_lossy(&config.stdout)
        .lines()
        .find_map(|s| {
            s.trim()
                .strip_prefix("inet ")?
                .split_whitespace()
                .next()?
                .parse::<Ipv4Addr>()
                .ok()
        })
        .ok_or("请先连接 Wi-Fi")?;
    Ok(SocketAddr::from((ip, 8769)))
}

pub fn install(app: &App) -> tauri::Result<()> {
    let item =
        |id: &str, label: &str, enabled| MenuItem::with_id(app, id, label, enabled, None::<&str>);
    let brand = item("app-name", "red-green", false)?;
    let summary = item(
        "app-sessions",
        &session_summary(&super::read_snapshot()),
        false,
    )?;
    let divider = PredefinedMenuItem::separator(app)?;
    let notice = item(
        "reader-notice",
        "配对设备可阅读当前 Claude Code 和 Codex CLI 会话",
        false,
    )?;
    let toggle = item("reader-toggle", "开启局域网阅读", true)?;
    let address = item("reader-address", "共享已关闭", false)?;
    let code = item("reader-code", "生成新的配对码", false)?;
    let allow_label = "允许配对设备回复 Codex";
    let replies = item("reader-replies", allow_label, false)?;
    let revoke = item("reader-revoke", "解除设备配对", false)?;
    let menu = Menu::with_items(
        app,
        &[
            &brand, &summary, &divider, &notice, &toggle, &address, &code, &replies, &revoke,
        ],
    )?;
    app.listen("sessions-changed", move |event| {
        if let Ok(sessions) = serde_json::from_str::<Vec<serde_json::Value>>(event.payload()) {
            let next = session_summary(&sessions);
            if summary.text().ok().as_ref() != Some(&next) {
                let _ = summary.set_text(next);
            }
        }
    });
    let service = Mutex::new(None::<Reader>);
    TrayIconBuilder::with_id("red-green")
        .icon(tauri::include_image!("icons/app-menu-template.png"))
        .icon_as_template(true)
        .tooltip("red-green · 会话桌面宠物")
        .menu(&menu)
        .on_menu_event(move |_, event| {
            let mut active = service.lock().unwrap();
            match event.id.as_ref() {
                "reader-toggle" => {
                    if let Some(reader) = active.take() {
                        reader.suspend();
                        // An in-flight source read may take time. Keep the macOS event loop responsive.
                        let _ = toggle.set_enabled(false);
                        let _ = toggle.set_text("正在关闭…");
                        let stopped = toggle.clone();
                        std::thread::spawn(move || {
                            drop(reader);
                            let _ = stopped.set_text("开启局域网阅读");
                            let _ = stopped.set_enabled(true);
                        });
                        let _ = address.set_text("共享已关闭");
                        let _ = code.set_text("生成新的配对码");
                        let _ = code.set_enabled(false);
                        let _ = revoke.set_enabled(false);
                        let _ = replies.set_enabled(false);
                        let _ = replies.set_text(allow_label);
                    } else {
                        let result = wifi_address().and_then(|addr| {
                            let source = LiveSource::new(
                                reader_snapshot,
                                super::status_dir(),
                                super::codex_home()
                                    .join("app-server-control/app-server-control.sock"),
                            );
                            Reader::with_store(
                                addr,
                                source,
                                Some(
                                    std::path::PathBuf::from(
                                        std::env::var_os("HOME").ok_or("未找到用户目录")?,
                                    )
                                    .join(".config/red-green/nomad-device"),
                                ),
                            )
                            .map_err(|e| e.to_string())
                        });
                        match result {
                            Ok(reader) => {
                                let _ = address.set_text(format!("http://{}/", reader.address()));
                                let _ = code
                                    .set_text(format!("配对码 {} · 点此换码", reader.new_code()));
                                let _ = code.set_enabled(true);
                                let _ = revoke.set_enabled(true);
                                let _ = replies.set_enabled(true);
                                let _ = toggle.set_text("关闭局域网阅读");
                                *active = Some(reader);
                            }
                            Err(error) => {
                                let _ = address.set_text(format!("无法开启：{error}"));
                            }
                        }
                    }
                }
                "reader-replies" => {
                    if let Some(reader) = active.as_ref() {
                        let label = match reader.toggle_replies() {
                            Some(true) => {
                                let _ = code.set_text("生成新的配对码");
                                "关闭设备回复 Codex"
                            }
                            Some(false) => allow_label,
                            None => "请先配对，再点此允许回复 Codex",
                        };
                        let _ = replies.set_text(label);
                    }
                }
                "reader-code" => {
                    if let Some(reader) = active.as_ref() {
                        let _ = code.set_text(format!("配对码 {} · 点此换码", reader.new_code()));
                        let _ = replies.set_text(allow_label);
                    }
                }
                "reader-revoke" => {
                    if let Some(reader) = active.as_ref() {
                        let label = match reader.revoke() {
                            Ok(()) => "已解除配对 · 点此生成配对码".into(),
                            Err(e) => format!("撤销未保存：{e}"),
                        };
                        let _ = code.set_text(label);
                        let _ = replies.set_text(allow_label);
                    }
                }
                _ => (),
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reader_shows_only_the_acknowledged_event_as_idle() {
        let mut sessions = vec![
            json!({"session_id":"one", "state":"completed", "since":"1"}),
            json!({"source":"codex", "session_id":"codex-two", "state":"aborted", "since":"2"}),
            json!({"session_id":"three", "state":"completed", "since":"new"}),
            json!({"session_id":"four", "state":"your_turn", "since":"4"}),
        ];
        let acknowledged = Acknowledged::from([
            ("claude-one".into(), "1".into()),
            ("codex-codex-two".into(), "2".into()),
            ("claude-three".into(), "old".into()),
            ("claude-four".into(), "4".into()),
        ]);
        apply_acknowledged(&mut sessions, &acknowledged);
        let states: Vec<_> = sessions
            .iter()
            .map(|s| s["state"].as_str().unwrap())
            .collect();
        assert_eq!(states, ["idle", "idle", "completed", "your_turn"]);
    }
}
