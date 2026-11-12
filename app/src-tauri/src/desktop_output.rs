use red_green::output::{model::OutputSnapshot, runtime::Desktop};
use std::collections::HashSet;
use std::process::{Child, Command};
use tauri::{AppHandle, Emitter, Manager};

pub struct DesktopOutput {
    app: AppHandle,
    players: Vec<Child>,
}
impl DesktopOutput {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            players: vec![],
        }
    }
    fn stop_audio(&mut self) -> Result<(), String> {
        for child in &mut self.players {
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                child.kill().map_err(|e| e.to_string())?;
                child.wait().map_err(|e| e.to_string())?;
            }
        }
        self.players.clear();
        Ok(())
    }
}
impl Desktop for DesktopOutput {
    fn reconcile(&mut self, snapshot: &OutputSnapshot) -> Result<(), String> {
        let wanted: HashSet<_> = snapshot
            .pets
            .iter()
            .map(|p| format!("pet-{}", p.sid))
            .collect();
        for (label, w) in self.app.webview_windows() {
            if label.starts_with("pet-") && !wanted.contains(&label) {
                w.close().map_err(|e| e.to_string())?;
            }
        }
        for pet in &snapshot.pets {
            super::ensure_pet(self.app.clone(), pet.sid.clone(), pet.slot)?;
            if let Some(w) = self.app.get_webview_window(&format!("pet-{}", pet.sid)) {
                w.emit("pet-update", &pet.model)
                    .map_err(|e| e.to_string())?;
                w.show().map_err(|e| e.to_string())?;
            }
        }
        if snapshot.desktop_usage.is_empty() {
            super::remove_gauge(self.app.clone())?;
        } else {
            super::ensure_gauge(self.app.clone())?;
            if let Some(w) = self.app.get_webview_window(super::GAUGE_LABEL) {
                w.emit("usage-update", &snapshot.desktop_usage)
                    .map_err(|e| e.to_string())?;
                w.show().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    fn quiesce(&mut self) -> Result<(), String> {
        let mut failed = None;
        for (label, w) in self.app.webview_windows() {
            if label.starts_with("pet-") || label == super::GAUGE_LABEL {
                if let Err(e) = w.hide() {
                    failed = Some(e.to_string());
                }
                match w.is_visible() {
                    Ok(false) => {}
                    Ok(true) => failed = Some("窗口仍然可见".into()),
                    Err(e) => failed = Some(e.to_string()),
                }
            }
        }
        self.stop_audio()?;
        failed.map_or(Ok(()), Err)
    }
    fn play(&mut self, name: &str) -> Result<(), String> {
        self.players
            .retain_mut(|p| !matches!(p.try_wait(), Ok(Some(_))));
        let path = self
            .app
            .path()
            .resolve(
                format!("calls/{name}.wav"),
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| e.to_string())?;
        self.players.push(
            Command::new("afplay")
                .arg(path)
                .spawn()
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
}
impl Drop for DesktopOutput {
    fn drop(&mut self) {
        let _ = self.stop_audio();
    }
}
