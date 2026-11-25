//! 客户端自己的标题绑定 thread → tty；不使用后台服务继承的终端环境变量。
use crate::{ghostty, osascript, tmux, tty_host, Host};
use red_green::codex::model;
use red_green::reader::live::ResumeTerminal;
use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

const NEW_GHOSTTY_TAB: &str = r#"on run argv
    set launchCommand to item 1 of argv
    set launchDirectory to item 2 of argv
    tell application "Ghostty"
        set cfg to new surface configuration
        set command of cfg to launchCommand
        set initial working directory of cfg to launchDirectory
        if (count windows) > 0 then
            set newTab to new tab in front window with configuration cfg
            try
                return id of focused terminal of newTab
            end try
        else
            set newWindow to new window with configuration cfg
            try
                return id of focused terminal of selected tab of newWindow
            end try
        end if
        return ""
    end tell
end run"#;

const NEW_TERMINAL_WINDOW: &str = r#"on run argv
    set launchCommand to item 1 of argv
    tell application "Terminal"
        set newTab to do script launchCommand
        try
            return tty of newTab
        end try
        return ""
    end tell
end run"#;

pub fn resume(cwd: &Path, id: &str, socket: &Path) -> Result<ResumeTerminal, u16> {
    if !model::is_thread_id(id) {
        return Err(400);
    }
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err(422);
    }
    let codex = codex_executable().ok_or(424u16)?;
    let command = resume_command(&codex, cwd, id, socket).ok_or(424u16)?;
    let open = |script: &str| {
        let output = Command::new("/usr/bin/osascript")
            .args(["-e", script, &command])
            .arg(cwd)
            .output()
            .ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else if String::from_utf8_lossy(&output.stderr).contains("(-1712)") {
            // Apple Event 超时仍可能已经创建了标签。保留未知凭证，避免再开一次。
            Some(String::new())
        } else {
            None
        }
    };
    if running("Ghostty") {
        if let Some(id) = open(NEW_GHOSTTY_TAB) {
            return Ok(if model::is_thread_id(&id) {
                ResumeTerminal::Ghostty(id)
            } else {
                ResumeTerminal::Unknown
            });
        }
    }
    // Terminal 的 do script 省略目标会创建新窗口；不会在用户选中的标签内执行。
    let tty = open(NEW_TERMINAL_WINDOW).ok_or(424u16)?;
    if tty.starts_with("/dev/tty") {
        if let Ok(metadata) = std::fs::metadata(&tty) {
            return Ok(ResumeTerminal::Terminal {
                tty,
                inode: metadata.ino(),
            });
        }
    }
    Ok(ResumeTerminal::Unknown)
}

pub fn terminal_exists(terminal: &ResumeTerminal) -> Result<bool, u16> {
    let (script, identity) = match terminal {
        ResumeTerminal::Ghostty(id) => (
            r#"on run argv
            if not (application "Ghostty" is running) then return false
            set surfaceId to item 1 of argv
            tell application "Ghostty" to return (exists terminal id surfaceId)
        end run"#,
            id,
        ),
        ResumeTerminal::Terminal { tty, inode } => {
            // devfs 会把刚释放的 tty 名给下一窗口。原设备已消失或换了 inode 即可重试。
            match std::fs::metadata(tty) {
                Ok(meta) if meta.ino() != *inode => return Ok(false),
                Ok(_) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(_) => return Err(424),
            }
            (
                r#"on run argv
            if not (application "Terminal" is running) then return false
            set device to item 1 of argv
            tell application "Terminal"
                repeat with w in windows
                    if (tty of tabs of w) contains device then return true
                end repeat
                return false
            end tell
        end run"#,
                tty,
            )
        }
        ResumeTerminal::Unknown => return Err(428),
    };
    match script_query(script, identity)?.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(424),
    }
}

fn script_query(script: &str, arg: &str) -> Result<String, u16> {
    let out = Command::new("/usr/bin/osascript")
        .args(["-e", script, arg])
        .output()
        .map_err(|_| 424u16)?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        .ok_or(424)
}

// 桌面快照可能恰好在刷新。只用真实 Codex TTY 上的标题，对照同一服务的已加载线程消歧。
pub fn thread_terminal_exists(id: &str, loaded: &[String]) -> Result<bool, u16> {
    if !model::is_thread_id(id) || !loaded.iter().any(|thread| thread == id) {
        return Ok(false);
    }
    let live = live_codex_ttys()?;
    if live.is_empty() {
        return Ok(false);
    }
    let mut rows = tmux(&["list-panes", "-a", "-F", "#{pane_tty}\t#{pane_title}"])
        .map_or_else(Vec::new, |out| parse_rows(&out));
    let mut uncertain = false;
    for (name, script) in [
        (
            "Terminal",
            r#"on run argv
            set prefixHint to item 1 of argv
            set output to ""
            tell application "Terminal"
                repeat with w in windows
                    repeat with t in tabs of w
                        set titleText to custom title of t
                        if titleText contains prefixHint then set output to output & tty of t & (ASCII character 9) & titleText & linefeed
                    end repeat
                end repeat
            end tell
            return output
        end run"#,
        ),
        (
            "Ghostty",
            r#"on run argv
            set prefixHint to item 1 of argv
            set output to ""
            tell application "Ghostty"
                repeat with t in terminals
                    set titleText to name of t
                    if titleText contains prefixHint then set output to output & («class Gtty» of t) & (ASCII character 9) & titleText & linefeed
                end repeat
            end tell
            return output
        end run"#,
        ),
    ] {
        match process_running(name) {
            Ok(false) => continue,
            Ok(true) => match script_query(script, &id[..29]) {
                Ok(out) => rows.extend(parse_rows(&out)),
                Err(_) => uncertain = true,
            },
            Err(_) => uncertain = true,
        }
    }
    for (hint, _) in model::bindings(&rows, &live) {
        if id.starts_with(&hint) {
            if model::resolve_id(&hint, loaded).as_deref() == Some(id) {
                return Ok(true);
            }
            uncertain = true;
        }
    }
    if uncertain {
        Err(424)
    } else {
        Ok(false)
    }
}

fn codex_executable() -> Option<PathBuf> {
    let executable = |path: &Path| {
        path.is_absolute()
            && path
                .metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(bin) = std::env::split_paths(&path)
        .map(|dir| dir.join("codex"))
        .find(|p| executable(p))
    {
        return Some(bin);
    }
    // launchd 下的 .app 没有用户终端的 PATH。通过登录 shell 找到用户安装的 CLI。
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let found = Command::new(shell)
        .args(["-lic", "/usr/bin/which codex"])
        .current_dir("/")
        .output()
        .ok()?;
    let out = std::str::from_utf8(&found.stdout).ok()?;
    let bin = PathBuf::from(out.lines().last()?);
    (found.status.success() && executable(&bin)).then_some(bin)
}

fn resume_command(codex: &Path, cwd: &Path, id: &str, socket: &Path) -> Option<String> {
    let home = socket.parent()?.parent()?;
    if !socket.is_absolute() {
        return None;
    }
    let mut endpoint = OsString::from("unix://");
    endpoint.push(socket);
    // 终端会按 shell 命令解释字符串。%q 在 C locale 下把换行、控制字节和引号
    // 都转为可打印的 Bash 参数；外层的单引号让用户自己的 shell 原样转交。
    let escaped = Command::new("/bin/bash")
        .env("LC_ALL", "C")
        .args([
            "-c",
            r#"printf 'cd -- %q && export CODEX_HOME=%q && exec %q resume --remote %q %q' "$@""#,
            "_",
        ])
        .args([
            cwd.as_os_str(),
            home.as_os_str(),
            codex.as_os_str(),
            &endpoint,
            OsStr::new(id),
        ])
        .output()
        .ok()?;
    let script = String::from_utf8(escaped.stdout).ok()?;
    escaped
        .status
        .success()
        .then(|| format!("/bin/bash -c '{}'", script.replace('\'', "'\\''")))
}

fn live_codex_ttys() -> Result<HashSet<String>, u16> {
    let output = Command::new("ps")
        .args(["-axo", "tty=,comm="])
        .output()
        .map_err(|_| 424u16)?;
    if !output.status.success() {
        return Err(424);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (tty, command) = line.trim().split_once(char::is_whitespace)?;
            (tty != "??" && std::path::Path::new(command.trim()).file_name()?.to_str()? == "codex")
                .then(|| format!("/dev/{tty}"))
        })
        .collect())
}

pub fn discover() -> BTreeMap<String, String> {
    let live = live_codex_ttys().unwrap_or_default();
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
    if running("Ghostty") {
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
    process_running(name).unwrap_or(false)
}

fn process_running(name: &str) -> Result<bool, u16> {
    let output = Command::new("pgrep")
        .args(["-ix", name])
        .output()
        .map_err(|_| 424u16)?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(424),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01234567-89ab-cdef-8123-456789abcdef";

    #[test]
    fn reused_tty_name_does_not_keep_a_closed_terminal_pending() {
        let path = std::env::temp_dir().join(format!("rg-tty-{}", rand::random::<u64>()));
        let original = std::fs::File::create_new(&path).unwrap();
        let terminal = ResumeTerminal::Terminal {
            tty: path.to_str().unwrap().into(),
            inode: original.metadata().unwrap().ino(),
        };
        std::fs::remove_file(&path).unwrap();
        assert_eq!(terminal_exists(&terminal), Ok(false));
        std::fs::File::create_new(&path).unwrap();
        assert_eq!(terminal_exists(&terminal), Ok(false));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_bad_id_and_missing_cwd_before_opening_a_terminal() {
        let socket = Path::new("/unused/app-server-control/app-server-control.sock");
        assert_eq!(terminal_exists(&ResumeTerminal::Unknown), Err(428));
        assert_eq!(resume(Path::new("/"), "--last", socket), Err(400));
        assert_eq!(resume(Path::new("relative"), ID, socket), Err(422));
        assert_eq!(resume(Path::new("/dev/null"), ID, socket), Err(422));
    }

    #[test]
    fn terminal_command_preserves_original_cwd_home_socket_and_id() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("rg-resume-{}", rand::random::<u64>()));
        let cwd = dir.join("測試 '$(echo bad)'\n");
        let home = dir.join("home \"$HOME\"\\");
        let socket = home.join("app-server-control/app-server-control.sock");
        fs::create_dir_all(&cwd).unwrap();
        let bin = dir.join("fake codex");
        fs::write(
            &bin,
            b"#!/bin/sh\nprintf '%s\\0' \"$PWD\" \"$CODEX_HOME\" \"$@\"\n",
        )
        .unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
        let command = resume_command(&bin, &cwd, ID, &socket).unwrap();
        assert!(!command.bytes().any(|b| b.is_ascii_control()));
        let out = Command::new("/bin/sh")
            .args(["-c", &command])
            .output()
            .unwrap();
        fs::remove_dir_all(dir).unwrap();
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8(out.stdout)
                .unwrap()
                .split_terminator('\0')
                .collect::<Vec<_>>(),
            [
                cwd.to_str().unwrap(),
                home.to_str().unwrap(),
                "resume",
                "--remote",
                &format!("unix://{}", socket.display()),
                ID
            ]
        );
    }
}
