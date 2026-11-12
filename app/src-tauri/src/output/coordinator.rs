#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Desktop,
    Preparing,
    Device,
    Fallback,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    None,
    Quiesce,
    Grant,
}

// 不读取墙上时钟；runtime 使用包含睡眠时间的单调毫秒。设备也有独立期限。
pub struct Coordinator {
    mode: Mode,
    safe_after: u64,
    model_at: Option<u64>,
    version: u64,
    first_health: Option<u64>,
    last_response: u64,
    last_nonce: u64,
    pending: Option<(u64, u64, u64)>,
    last_sent: Option<u64>,
}
impl Coordinator {
    pub fn new(now: u64) -> Self {
        Self {
            mode: Mode::Fallback,
            safe_after: now + 2500,
            model_at: None,
            version: 0,
            first_health: None,
            last_response: now,
            last_nonce: 0,
            pending: None,
            last_sent: None,
        }
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn model(&mut self, now: u64, version: u64) {
        self.model_at = Some(now);
        self.version = version;
    }
    fn fresh(&self, now: u64) -> bool {
        self.model_at.is_some_and(|t| now.saturating_sub(t) <= 3000)
    }
    pub fn ready(&mut self, now: u64, compatible: bool) -> bool {
        self.tick(now);
        if !compatible || self.mode != Mode::Desktop || !self.fresh(now) {
            return false;
        }
        self.mode = Mode::Preparing;
        self.first_health = None;
        self.last_nonce = 0;
        self.pending = None;
        self.last_response = now;
        true
    }
    pub fn request(&mut self, now: u64, nonce: u64, version: u64, healthy: bool) -> Decision {
        self.tick(now);
        if !matches!(self.mode, Mode::Preparing | Mode::Device) {
            return Decision::None;
        }
        if !healthy || !self.fresh(now) {
            self.disconnect(now);
            return Decision::None;
        }
        if nonce <= self.last_nonce {
            return Decision::None;
        }
        self.pending = None;
        self.last_nonce = nonce;
        self.last_response = now;
        let first = *self.first_health.get_or_insert(now);
        if version != self.version || now.saturating_sub(first) < 500 {
            return Decision::None;
        }
        self.pending = Some((nonce, version, now));
        if self.mode == Mode::Device {
            Decision::Grant
        } else {
            Decision::Quiesce
        }
    }
    pub fn grant(&mut self, now: u64, stopped: bool) -> Option<(u64, u64)> {
        let (nonce, version, received) = self.pending.take()?;
        if !stopped
            || !self.fresh(now)
            || version != self.version
            || now.saturating_sub(received) > 100
        {
            return None;
        }
        self.mode = Mode::Device;
        self.sent(now); // 写失败也必须算作“可能已经送达”。
        Some((nonce, version))
    }
    pub fn sent(&mut self, now: u64) {
        self.last_sent = Some(now);
        self.safe_after = now + 2500;
    }
    pub fn disconnect(&mut self, now: u64) {
        self.pending = None;
        self.first_health = None;
        self.mode = if now < self.safe_after {
            Mode::Fallback
        } else {
            Mode::Desktop
        };
    }
    pub fn tick(&mut self, now: u64) {
        if matches!(self.mode, Mode::Preparing | Mode::Device)
            && (!self.fresh(now) || now.saturating_sub(self.last_response) > 1200)
        {
            self.disconnect(now);
        }
        if self.mode == Mode::Fallback && now >= self.safe_after {
            self.mode = Mode::Desktop;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prepared() -> Coordinator {
        let mut c = Coordinator::new(0);
        c.tick(2500);
        c.model(2500, 1);
        assert!(c.ready(2500, true));
        assert_eq!(c.request(2500, 1, 1, true), Decision::None);
        assert_eq!(c.request(3000, 2, 1, true), Decision::Quiesce);
        c
    }
    #[test]
    fn startup_and_incompatible_device_never_take_over() {
        let mut c = Coordinator::new(0);
        c.tick(2499);
        assert_eq!(c.mode(), Mode::Fallback);
        c.tick(2500);
        assert_eq!(c.mode(), Mode::Desktop);
        c.model(2500, 1);
        assert!(!c.ready(2500, false));
        assert_eq!(c.request(3000, 1, 1, true), Decision::None);
    }
    #[test]
    fn requires_two_spaced_health_reports_and_successful_desktop_stop() {
        let mut c = prepared();
        assert_eq!(c.grant(3000, false), None);
        assert_eq!(c.mode(), Mode::Preparing);
        c.request(3500, 3, 1, true);
        assert_eq!(c.grant(3500, true), Some((3, 1)));
        assert_eq!(c.mode(), Mode::Device);
    }
    #[test]
    fn disconnect_waits_from_last_possible_send_and_does_not_grant_old_nonce() {
        let mut c = prepared();
        c.grant(3000, true);
        c.sent(3050);
        assert_eq!(c.request(3200, 2, 1, true), Decision::None);
        c.disconnect(3300);
        c.tick(5549);
        assert_eq!(c.mode(), Mode::Fallback);
        c.tick(5550);
        assert_eq!(c.mode(), Mode::Desktop);
        assert_eq!(c.grant(5550, true), None);
    }
    #[test]
    fn stale_model_or_unhealthy_renderer_revokes_renewal() {
        let mut c = prepared();
        c.grant(3000, true);
        c.request(3500, 3, 1, true);
        assert_eq!(c.grant(3500, true), Some((3, 1)));
        assert_eq!(c.request(5501, 4, 1, true), Decision::None);
        assert_eq!(c.mode(), Mode::Fallback);
        let mut c = prepared();
        c.grant(3000, true);
        assert_eq!(c.request(3500, 3, 1, false), Decision::None);
        assert_eq!(c.mode(), Mode::Fallback);
    }
    #[test]
    fn unapplied_snapshot_and_replayed_nonce_do_not_allow_takeover() {
        let mut c = Coordinator::new(0);
        c.tick(2500);
        c.model(2500, 2);
        c.ready(2500, true);
        c.request(2500, 1, 1, true);
        assert_eq!(c.request(3000, 1, 2, true), Decision::None);
        assert_eq!(c.request(3001, 2, 1, true), Decision::None);
        assert_eq!(c.grant(3001, true), None);
        assert_eq!(c.request(3500, 3, 2, true), Decision::Quiesce);
    }
}
