use super::coordinator::{Coordinator, Decision, Mode};
use super::model::opaque_id;
use super::model::{Call, OutputSnapshot};
use serde_json::json;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{Receiver, SyncSender},
};
use std::time::Duration;
pub const RESOURCE_VERSION: &str = "red-green-assets-v1";
pub trait Desktop {
    fn reconcile(&mut self, snapshot: &OutputSnapshot) -> Result<(), String>;
    fn quiesce(&mut self) -> Result<(), String>;
    fn play(&mut self, name: &str) -> Result<(), String>;
}
pub struct Engine<D: Desktop> {
    pub desktop: D,
    coordinator: Coordinator,
    snapshot: OutputSnapshot,
    version: u64,
    generation: String,
    boot: Option<String>,
    dirty: bool,
    event: u64,
    nonce: u64,
}
impl<D: Desktop> Engine<D> {
    pub fn new(desktop: D, now: u64) -> Self {
        Self {
            desktop,
            coordinator: Coordinator::new(now),
            snapshot: OutputSnapshot::default(),
            version: 0,
            generation: String::new(),
            boot: None,
            dirty: true,
            event: 0,
            nonce: 0,
        }
    }
    pub fn mode(&self) -> Mode {
        self.coordinator.mode()
    }
    pub fn has_peer(&self) -> bool {
        self.boot.is_some()
    }
    pub fn refresh(&mut self) {
        self.dirty = true;
    }
    pub fn sent(&mut self, now: u64) {
        self.coordinator.sent(now);
    }
    fn visible(&self) -> bool {
        matches!(self.mode(), Mode::Desktop | Mode::Preparing)
    }
    pub fn submit(
        &mut self,
        s: OutputSnapshot,
        calls: Vec<Call>,
        now: u64,
        submitted: u64,
    ) -> Result<Vec<Value>, String> {
        let changed = self.snapshot != s || self.version == 0;
        self.snapshot = s;
        if changed {
            self.version += 1;
            self.dirty = true;
        }
        self.coordinator.model(submitted, self.version);
        self.tick(now)?;
        let wire = match self.snapshot.wire(&self.generation, self.version) {
            Ok(w) => w,
            Err(e) => {
                self.disconnected(now);
                return Err(e);
            }
        };
        let mut out = vec![];
        if changed && self.has_peer() {
            out.push(wire);
        }
        if now.saturating_sub(submitted) > 2000 {
            return Ok(out);
        }
        for call in calls {
            let Some(pet) = self.snapshot.pets.iter().find(|p| p.sid == call.sid) else {
                continue;
            };
            let Some((skin, state)) = call.name.split_once('-') else {
                continue;
            };
            if !["robot", "crab", "cat"].contains(&skin)
                || !["awaiting", "aborted", "your_turn", "completed"].contains(&state)
                || pet.model["skin"] != skin
            {
                continue;
            }
            self.event += 1;
            if self.mode() == Mode::Device {
                out.push(json!({"type":"call","boot":self.boot,"generation":self.generation,"nonce":self.nonce,"event":self.event,"session":opaque_id(&call.sid),"name":call.name}));
            } else if self.visible() {
                self.desktop.play(&call.name)?;
            }
            // 交接/回退期间的叫声丢弃，不能稍后补发。
        }
        Ok(out)
    }
    pub fn message(&mut self, message: Value, now: u64) -> Result<Vec<Value>, String> {
        match message["type"].as_str() {
            Some("hello") => {
                let compatible = message["app"] == super::protocol::APP_ID
                    && message["protocol"] == super::protocol::PROTOCOL_VERSION
                    && message["assets"] == RESOURCE_VERSION
                    && message["board"] == "v1.2"
                    && message["display_ok"] == true
                    && message["audio_ok"] == true;
                if !compatible {
                    return Err("Mosaico 应用、资源或硬件不兼容".into());
                }
                let boot = message["boot"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 64)
                    .ok_or("设备启动 ID 无效")?;
                if self.boot.as_deref() == Some(boot) {
                    return Ok(vec![]);
                }
                if self.has_peer() {
                    self.disconnected(now);
                    return Err("设备启动代次变化".into());
                }
                if !self.coordinator.ready(now, true) {
                    return Ok(vec![]);
                }
                static GENERATIONS: AtomicU64 = AtomicU64::new(1);
                self.generation = format!(
                    "g{now:x}-{:x}-{:x}",
                    std::process::id(),
                    GENERATIONS.fetch_add(1, Ordering::Relaxed)
                );
                self.boot = Some(boot.into());
                self.nonce = 0;
                let snapshot = self.snapshot.wire(&self.generation, self.version)?;
                Ok(vec![
                    json!({"type":"bind","boot":boot,"generation":self.generation,"protocol":1,"assets":RESOURCE_VERSION}),
                    snapshot,
                ])
            }
            Some("lease_request") => {
                if !self.has_peer()
                    || message["generation"] != self.generation
                    || message["boot"].as_str() != self.boot.as_deref()
                {
                    return Ok(vec![]);
                }
                let nonce = message["nonce"].as_u64().ok_or("nonce 无效")?;
                let version = message["version"].as_u64().ok_or("快照版本无效")?;
                let decision =
                    self.coordinator
                        .request(now, nonce, version, message["healthy"] == true);
                if decision == Decision::None {
                    return Ok(vec![]);
                }
                let stopped = decision == Decision::Grant || self.desktop.quiesce().is_ok();
                if let Some((nonce, version)) = self.coordinator.grant(now, stopped) {
                    self.nonce = nonce;
                    return Ok(vec![
                        json!({"type":"grant","boot":self.boot,"generation":self.generation,"nonce":nonce,"version":version}),
                    ]);
                }
                self.dirty = true;
                self.tick(now)?;
                Ok(vec![])
            }
            Some("release") => {
                if message["generation"] == self.generation
                    && message["boot"].as_str() == self.boot.as_deref()
                {
                    self.disconnected(now);
                }
                Ok(vec![])
            }
            Some("applied") => Ok(vec![]),
            _ => Err("未知设备消息".into()),
        }
    }
    pub fn tick(&mut self, now: u64) -> Result<(), String> {
        let previous = self.mode();
        self.coordinator.tick(now);
        if self.mode() != previous {
            self.dirty = true;
        }
        if matches!(self.mode(), Mode::Desktop | Mode::Fallback) {
            self.boot = None;
        }
        if self.visible() && self.dirty {
            self.desktop.reconcile(&self.snapshot)?;
            self.dirty = false;
        }
        Ok(())
    }
    pub fn disconnected(&mut self, now: u64) {
        self.coordinator.disconnect(now);
        self.boot = None;
        self.dirty = true;
    }
}

pub enum Command {
    Submit {
        snapshot: OutputSnapshot,
        calls: Vec<Call>,
        at: u64,
    },
    Refresh,
}
pub struct Control(pub SyncSender<Command>);

// Darwin RAW 包含睡眠时间；UPTIME_RAW/Instant 在睡眠期间停走，不能用于授权。
pub fn now_ms() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    #[cfg(target_os = "macos")]
    let clock = libc::CLOCK_MONOTONIC_RAW;
    #[cfg(not(target_os = "macos"))]
    let clock = libc::CLOCK_BOOTTIME;
    unsafe {
        libc::clock_gettime(clock, &mut ts);
    }
    ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000
}

pub fn run<D: Desktop>(desktop: D, rx: Receiver<Command>, log: impl Fn(&str)) {
    use super::transport::Link;
    let mut engine = Engine::new(desktop, now_ms());
    let mut link: Option<Link> = None;
    let mut next_scan = 0;
    let mut opened = 0;
    let mut last_mode = engine.mode();
    let mut last_error = String::new();
    loop {
        let started = now_ms();
        let result = (|| -> Result<(), String> {
            let had_peer = engine.has_peer();
            engine.tick(started)?;
            if had_peer && !engine.has_peer() {
                link = None;
                next_scan = started + 1000;
            }
            // 有界通道；处理量也有界，不允许快照洪水饿死授权到期检查。
            for _ in 0..8 {
                match rx.try_recv() {
                    Ok(Command::Submit {
                        snapshot,
                        calls,
                        at,
                    }) => {
                        let out = engine.submit(snapshot, calls, now_ms(), at)?;
                        if let Some(l) = link.as_mut() {
                            for v in out {
                                l.send(&v)?;
                            }
                        }
                    }
                    Ok(Command::Refresh) => engine.refresh(),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        return Err("输出控制通道已关闭".into())
                    }
                }
            }
            if link.is_none() && engine.mode() == Mode::Desktop && started >= next_scan {
                next_scan = started + 1000;
                if let Some(path) = super::transport::discover().into_iter().next() {
                    link = Some(Link::open(&path)?);
                    opened = started;
                }
            }
            if let Some(l) = link.as_mut() {
                for message in l.read()? {
                    for v in engine.message(message, now_ms())? {
                        let grant = v["type"] == "grant";
                        let result = l.send(&v);
                        if grant {
                            engine.sent(now_ms());
                        } // 包括失败与可能部分写入的时刻。
                        result?;
                    }
                }
                if !engine.has_peer() && now_ms().saturating_sub(opened) > 2000 {
                    return Err("设备应用未就绪".into());
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            if e != last_error {
                log(&format!("mosaico: {e}"));
                last_error = e;
            }
            engine.disconnected(now_ms());
            link = None;
            next_scan = now_ms() + 1000;
        }
        if engine.mode() != last_mode {
            log(&format!("mosaico output: {:?}", engine.mode()));
            last_mode = engine.mode();
        }
        std::thread::sleep(Duration::from_millis(
            50u64.saturating_sub(now_ms().saturating_sub(started)),
        ));
    }
}
#[cfg(test)]
mod tests {
    use super::super::model::Pet;
    use super::*;
    use serde_json::json;
    #[derive(Default)]
    struct Screen {
        visible: bool,
        ids: Vec<String>,
        calls: usize,
        fail_hide: bool,
    }
    impl Desktop for Screen {
        fn reconcile(&mut self, s: &OutputSnapshot) -> Result<(), String> {
            self.visible = true;
            self.ids = s.pets.iter().map(|p| p.sid.clone()).collect();
            Ok(())
        }
        fn quiesce(&mut self) -> Result<(), String> {
            if self.fail_hide {
                return Err("hide failed".into());
            }
            self.visible = false;
            Ok(())
        }
        fn play(&mut self, _: &str) -> Result<(), String> {
            self.calls += 1;
            Ok(())
        }
    }
    fn snapshot(id: &str) -> OutputSnapshot {
        OutputSnapshot {
            pets: vec![Pet {
                sid: id.into(),
                slot: 0,
                model: json!({"project":"工具箱","source":"codex","skin":"cat","state":"running"}),
            }],
            ..Default::default()
        }
    }
    fn call(id: &str) -> Call {
        Call {
            sid: id.into(),
            name: "cat-awaiting".into(),
        }
    }
    fn hello() -> Value {
        json!({"type":"hello","app":"red-green.mosaico","protocol":1,"assets":RESOURCE_VERSION,"boot":"boot1","board":"v1.2","display_ok":true,"audio_ok":true})
    }
    fn request(g: &str, nonce: u64) -> Value {
        json!({"type":"lease_request","boot":"boot1","generation":g,"nonce":nonce,"version":1,"healthy":true})
    }
    fn connect(e: &mut Engine<Screen>) -> String {
        e.submit(snapshot("a"), vec![], 2500, 2500).unwrap();
        e.tick(2500).unwrap();
        let out = e.message(hello(), 2500).unwrap();
        let g = out.iter().find(|v| v["type"] == "bind").unwrap()["generation"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(e.message(request(&g, 1), 2500).unwrap().is_empty());
        g
    }
    #[test]
    fn hides_before_grant_and_restores_only_latest_snapshot_without_replay() {
        let mut e = Engine::new(Screen::default(), 0);
        let g = connect(&mut e);
        assert!(e.desktop.visible);
        let out = e.message(request(&g, 2), 3000).unwrap();
        assert!(out.iter().any(|v| v["type"] == "grant"));
        assert!(!e.desktop.visible);
        let calls = e
            .submit(snapshot("b"), vec![call("b")], 3200, 3200)
            .unwrap();
        assert!(!e.desktop.visible);
        assert_eq!(e.desktop.calls, 0);
        assert_eq!(calls.iter().filter(|v| v["type"] == "call").count(), 1);
        e.disconnected(3300);
        e.tick(5499).unwrap();
        assert!(!e.desktop.visible);
        e.tick(5500).unwrap();
        assert!(e.desktop.visible);
        assert_eq!(e.desktop.ids, vec!["b"]);
        assert_eq!(e.desktop.calls, 0);
    }
    #[test]
    fn failed_hide_and_wrong_generation_never_authorize() {
        let mut e = Engine::new(
            Screen {
                fail_hide: true,
                ..Default::default()
            },
            0,
        );
        let g = connect(&mut e);
        assert!(e
            .message(request("old-generation", 2), 3000)
            .unwrap()
            .is_empty());
        assert!(e.message(request(&g, 2), 3000).unwrap().is_empty());
        assert!(e.desktop.visible);
        e.submit(snapshot("a"), vec![call("a")], 3100, 3100)
            .unwrap();
        assert_eq!(e.desktop.calls, 1);
    }
    #[test]
    fn stale_queued_calls_and_mismatched_resources_stay_silent() {
        let mut e = Engine::new(Screen::default(), 0);
        e.tick(2500).unwrap();
        e.submit(snapshot("a"), vec![call("a")], 5000, 2500)
            .unwrap();
        assert_eq!(e.desktop.calls, 0);
        let mut h = hello();
        h["assets"] = json!("old");
        assert!(e.message(h, 5000).is_err());
        assert!(e.desktop.visible);
    }
}
