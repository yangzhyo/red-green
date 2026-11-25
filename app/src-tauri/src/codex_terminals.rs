//! 客户端自己的标题绑定 thread → tty；不使用后台服务继承的终端环境变量。
use crate::{ghostty, osascript, tmux, tty_host, Host};
use red_green::codex::model;
use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
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
            new tab in front window with configuration cfg
        else
            new window with configuration cfg
        end if
    end tell
end run"#;

const NEW_TERMINAL_WINDOW: &str = r#"on run argv
    set launchCommand to item 1 of argv
    tell application "Terminal" to do script launchCommand
end run"#;

pub fn resume(cwd: &Path, id: &str, socket: &Path) -> Result<(), u16> {
    if !model::is_thread_id(id) {
        return Err(400);
    }
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err(422);
    }
    let codex = codex_executable().ok_or(424u16)?;
    let command = resume_command(&codex, cwd, id, socket).ok_or(424u16)?;
    let open = |script: &str| {
        Command::new("/usr/bin/osascript")
            .args(["-e", script, &command])
            .arg(cwd)
            .output()
            .is_ok_and(|out| out.status.success())
    };
    if (running("ghostty") || running("Ghostty")) && open(NEW_GHOSTTY_TAB) {
        return Ok(());
    }
    // Terminal 的 do script 省略目标会创建新窗口；不会在用户选中的标签内执行。
    open(NEW_TERMINAL_WINDOW).then_some(()).ok_or(424)
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

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01234567-89ab-cdef-8123-456789abcdef";

    #[test]
    fn rejects_bad_id_and_missing_cwd_before_opening_a_terminal() {
        let socket = Path::new("/unused/app-server-control/app-server-control.sock");
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
