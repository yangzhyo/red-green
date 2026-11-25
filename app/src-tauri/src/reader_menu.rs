use red_green::reader::{live::LiveSource, Reader};
use std::{
    net::{Ipv4Addr, SocketAddr},
    process::Command,
    sync::Mutex,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    App,
};

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
    let notice = item(
        "reader-notice",
        "配对设备可阅读当前 Claude Code 和 Codex CLI 会话",
        false,
    )?;
    let toggle = item("reader-toggle", "开启局域网阅读", true)?;
    let address = item("reader-address", "共享已关闭", false)?;
    let code = item("reader-code", "生成新的配对码", false)?;
    let revoke = item("reader-revoke", "解除设备配对", false)?;
    let menu = Menu::with_items(app, &[&notice, &toggle, &address, &code, &revoke])?;
    let service = Mutex::new(None::<Reader>);
    TrayIconBuilder::with_id("nomad-reader")
        .title("读")
        .tooltip("red-green 会话阅读")
        .menu(&menu)
        .on_menu_event(move |_, event| {
            let mut active = service.lock().unwrap();
            match event.id.as_ref() {
                "reader-toggle" => {
                    if let Some(reader) = active.take() {
                        reader.revoke();
                        // An in-flight source read may take time. Keep the macOS event loop responsive.
                        std::thread::spawn(move || drop(reader));
                        let _ = toggle.set_text("开启局域网阅读");
                        let _ = address.set_text("共享已关闭");
                        let _ = code.set_text("生成新的配对码");
                        let _ = code.set_enabled(false);
                        let _ = revoke.set_enabled(false);
                    } else {
                        let result = wifi_address().and_then(|addr| {
                            let source = LiveSource::new(
                                super::read_snapshot,
                                super::status_dir(),
                                super::codex_home()
                                    .join("app-server-control/app-server-control.sock"),
                            );
                            Reader::start(addr, source).map_err(|e| e.to_string())
                        });
                        match result {
                            Ok(reader) => {
                                let _ = address.set_text(format!("http://{}/", reader.address()));
                                let _ = code
                                    .set_text(format!("配对码 {} · 点此换码", reader.new_code()));
                                let _ = code.set_enabled(true);
                                let _ = revoke.set_enabled(true);
                                let _ = toggle.set_text("关闭局域网阅读");
                                *active = Some(reader);
                            }
                            Err(error) => {
                                let _ = address.set_text(format!("无法开启：{error}"));
                            }
                        }
                    }
                }
                "reader-code" => {
                    if let Some(reader) = active.as_ref() {
                        let _ = code.set_text(format!("配对码 {} · 点此换码", reader.new_code()));
                    }
                }
                "reader-revoke" => {
                    if let Some(reader) = active.as_ref() {
                        reader.revoke();
                        let _ = code.set_text("已解除配对 · 点此生成配对码");
                    }
                }
                _ => (),
            }
        })
        .build(app)?;
    Ok(())
}
