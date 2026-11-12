use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use toml_edit::{Array, DocumentMut, Item, Table};

pub fn configure_file(path: &Path) -> Result<Option<PathBuf>, String> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("config.toml 是符号链接，请手动添加 tui.terminal_title 中的 thread-id".into());
    }
    let original = match fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let output = configure_title(original.as_deref().unwrap_or(""))?;
    if original.as_deref() == Some(&output) {
        return Ok(None);
    }
    fs::create_dir_all(path.parent().ok_or("配置文件缺少父目录")?).map_err(|e| e.to_string())?;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup = original
        .as_ref()
        .map(|_| path.with_extension(format!("toml.red-green-{suffix}.bak")));
    if let Some(backup) = &backup {
        fs::copy(path, backup).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension(format!("toml.red-green-{suffix}.tmp"));
    let result = (|| -> std::io::Result<()> {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        if let Ok(metadata) = fs::metadata(path) {
            f.set_permissions(metadata.permissions())?;
        }
        f.write_all(output.as_bytes())?;
        f.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| e.to_string())?;
    Ok(backup)
}

pub fn run() -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or("HOME 未设置")?;
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".codex"));
    fs::create_dir_all(&codex_home).map_err(|e| e.to_string())?;
    let codex_home = fs::canonicalize(codex_home).map_err(|e| e.to_string())?;
    if let Some(backup) = configure_file(&codex_home.join("config.toml"))? {
        println!("原配置已备份：{}", backup.display());
    }
    let registration = PathBuf::from(home).join(".config/red-green");
    fs::create_dir_all(&registration).map_err(|e| e.to_string())?;
    fs::write(
        registration.join("codex-home"),
        codex_home.as_os_str().as_encoded_bytes(),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Codex 接入已配置：{}。重启 red-green 后，直接使用 codex 启动新会话。",
        codex_home.display()
    );
    Ok(())
}

pub fn configure_title(input: &str) -> Result<String, String> {
    let mut doc = input.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    if !doc.contains_key("tui") {
        doc["tui"] = Item::Table(Table::new());
    }
    let tui = doc["tui"].as_table_like_mut().ok_or("tui 必须是 TOML 表")?;
    if !tui.contains_key("terminal_title") {
        let mut a = Array::new();
        for item in ["activity", "thread-name", "project-name"] {
            a.push(item);
        }
        tui.insert("terminal_title", Item::Value(a.into()));
    }
    let a = tui
        .get_mut("terminal_title")
        .and_then(Item::as_array_mut)
        .ok_or("tui.terminal_title 必须是数组")?;
    if a.iter().any(|v| !v.is_str()) {
        return Err("terminal_title 中的项目必须是字符串".into());
    }
    let positions: Vec<_> = a
        .iter()
        .enumerate()
        .filter_map(|(i, v)| matches!(v.as_str(), Some("thread-id" | "session-id")).then_some(i))
        .collect();
    if positions == [0] && a.get(0).and_then(|v| v.as_str()) == Some("thread-id") {
        return Ok(input.into());
    }
    for i in positions.into_iter().rev() {
        a.remove(i);
    }
    a.insert(0, "thread-id");
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_install_backs_up_once_and_preserves_invalid_files() {
        let dir = std::env::temp_dir().join(format!("red-green-install-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "# original\nmodel='x'\n").unwrap();
        let backup = configure_file(&path).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "# original\nmodel='x'\n"
        );
        assert!(configure_file(&path).unwrap().is_none());
        std::fs::write(&path, "tui = 1").unwrap();
        assert!(configure_file(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "tui = 1");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preserves_comments_unknown_settings_and_existing_title_items() {
        let input="# keep me\nmodel = 'example'\n[tui]\n# my title\nterminal_title = ['project-name', 'activity']\n[other]\nvalue = 3\n";
        let output = configure_title(input).unwrap();
        let parsed = output.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(
            parsed["tui"]["terminal_title"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>(),
            ["thread-id", "project-name", "activity"]
        );
        assert!(output.contains("# keep me"));
        assert!(output.contains("# my title"));
        assert_eq!(parsed["other"]["value"].as_integer(), Some(3));
        assert_eq!(configure_title(&output).unwrap(), output);
    }

    #[test]
    fn accepts_new_config_inline_tui_and_legacy_session_id() {
        for input in [
            "",
            "tui = {terminal_title = []}\n",
            "[tui]\nterminal_title=['project','session-id']\n",
        ] {
            let output = configure_title(input).unwrap();
            let parsed = output.parse::<toml_edit::DocumentMut>().unwrap();
            let a = parsed["tui"]["terminal_title"].as_array().unwrap();
            assert_eq!(a.get(0).and_then(|v| v.as_str()), Some("thread-id"));
            assert_eq!(
                a.iter()
                    .filter(|v| matches!(v.as_str(), Some("thread-id" | "session-id")))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn rejects_invalid_config_without_repairing_user_settings() {
        for input in ["broken = [", "tui=1", "[tui]\nterminal_title='project'"] {
            assert!(configure_title(input).is_err());
        }
    }
}
