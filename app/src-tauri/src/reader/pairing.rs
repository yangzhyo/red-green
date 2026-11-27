use rand::Rng;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};

#[derive(Default)]
pub(super) struct Pairing {
    reply_tickets: Option<std::collections::VecDeque<(String, String)>>,
    forms: std::collections::VecDeque<(String, Instant)>,
    challenge: Option<(String, Instant, u8)>,
    fingerprints: BTreeSet<String>,
    store: Option<PathBuf>,
}

impl Pairing {
    pub fn load(store: Option<PathBuf>) -> io::Result<Self> {
        // One digest per line also accepts the original single-device file as-is.
        let fingerprints = match store.as_ref().map(fs::read_to_string).transpose() {
            Ok(Some(value)) => value.lines().map(str::to_owned).collect(),
            Ok(None) => BTreeSet::new(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            store,
            fingerprints,
            ..Self::default()
        })
    }

    pub fn revoke(&mut self) -> io::Result<()> {
        self.forms.clear();
        self.challenge = None;
        self.fingerprints.clear();
        self.reply_tickets = None;
        if let Some(path) = &self.store {
            match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => (),
            }
        }
        Ok(())
    }

    fn remember(&mut self, token: &str) -> io::Result<()> {
        let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
        if let Some(path) = &self.store {
            fs::create_dir_all(
                path.parent()
                    .ok_or_else(|| io::Error::other("配对目录无效"))?,
            )?;
            let temp = path.with_extension(format!("tmp-{:x}", rand::random::<u64>()));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            if let Err(e) = self
                .fingerprints
                .iter()
                .chain(std::iter::once(&digest))
                .try_for_each(|hash| writeln!(file, "{hash}"))
                .and_then(|()| fs::rename(&temp, path))
            {
                let _ = fs::remove_file(temp);
                return Err(e);
            }
        }
        self.fingerprints.insert(digest);
        Ok(())
    }
    pub fn new_form(&mut self) -> String {
        let nonce = format!("{:032x}", rand::random::<u128>());
        if self.forms.len() >= 32 {
            self.forms.pop_front();
        }
        self.forms
            .push_back((nonce.clone(), Instant::now() + Duration::from_secs(600)));
        nonce
    }

    pub fn consume_form(&mut self, nonce: &str) -> bool {
        self.forms
            .retain(|(_, deadline)| *deadline > Instant::now());
        let Some(index) = self.forms.iter().position(|(value, _)| value == nonce) else {
            return false;
        };
        self.forms.remove(index);
        true
    }

    pub fn issue(&mut self) -> String {
        self.reply_tickets = None;
        let code = format!("{:06}", rand::rng().random_range(0..1_000_000));
        self.challenge = Some((code.clone(), Instant::now() + Duration::from_secs(600), 5));
        code
    }
    pub fn exchange(&mut self, input: &str) -> io::Result<Option<String>> {
        let Some((code, deadline, attempts)) = self.challenge.as_mut() else {
            return Ok(None);
        };
        if *attempts == 0 || Instant::now() >= *deadline {
            return Ok(None);
        }
        if code != input {
            *attempts -= 1;
            return Ok(None);
        }
        let token = format!(
            "{:032x}{:032x}",
            rand::random::<u128>(),
            rand::random::<u128>()
        );
        self.remember(&token)?;
        self.challenge = None;
        Ok(Some(token))
    }
    pub fn toggle_replies(&mut self) -> Option<bool> {
        if self.fingerprints.is_empty() {
            return None;
        }
        if self.reply_tickets.take().is_some() {
            return Some(false);
        }
        // Close enrollment before granting replies to all currently paired readers.
        self.challenge = None;
        self.reply_tickets = Some(Default::default());
        Some(true)
    }

    pub fn replies_enabled(&self) -> bool {
        self.reply_tickets.is_some()
    }

    pub fn reply_ticket(&mut self, key: &str) -> Option<&str> {
        let tickets = self.reply_tickets.as_mut()?;
        // A paired reader can visit thousands of old threads. Keep only recent tickets.
        let index = tickets.iter().position(|(scope, _)| scope == key);
        let entry = index
            .and_then(|i| tickets.remove(i))
            .unwrap_or_else(|| (key.to_owned(), format!("{:032x}", rand::random::<u128>())));
        if tickets.len() == 128 {
            tickets.pop_front();
        }
        tickets.push_back(entry);
        tickets.back().map(|(_, token)| token.as_str())
    }

    pub fn check_ticket(&self, key: &str, token: &str) -> Result<(), u16> {
        let tickets = self.reply_tickets.as_ref().ok_or(403u16)?;
        if tickets
            .iter()
            .any(|(scope, issued)| scope == key && issued == token)
        {
            Ok(())
        } else {
            Err(412)
        }
    }

    pub fn take_ticket(&mut self, key: &str, token: &str) -> Result<(), u16> {
        self.check_ticket(key, token)?;
        self.reply_tickets
            .as_mut()
            .unwrap()
            .retain(|(scope, _)| scope != key);
        Ok(())
    }

    pub fn allows(&self, cookies: &str) -> bool {
        cookies
            .split(';')
            .filter_map(|part| part.trim().strip_prefix("nomad_reader="))
            .any(|value| {
                value.len() == 64
                    && self
                        .fingerprints
                        .contains(&format!("{:x}", Sha256::digest(value.as_bytes())))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browsing_many_threads_keeps_recent_action_tickets_valid() {
        let mut device = Pairing::default();
        let code = device.issue();
        device.exchange(&code).unwrap().unwrap();
        device.toggle_replies();
        let first = device.reply_ticket("resume/first").unwrap().to_owned();
        let active = device.reply_ticket("live").unwrap().to_owned();
        for i in 0..200 {
            device.reply_ticket(&format!("resume/{i}"));
            assert_eq!(device.reply_ticket("live"), Some(active.as_str()));
        }
        assert_eq!(device.check_ticket("resume/first", &first), Err(412));
        assert_eq!(device.take_ticket("live", &active), Ok(()));
    }

    #[test]
    fn legacy_and_new_devices_survive_restart_until_revoked() {
        let path = std::env::temp_dir().join(format!("rg-pair-{:x}", rand::random::<u64>()));
        let old_token = "a".repeat(64);
        // The previous release stored a single hex digest without a trailing newline.
        let old_digest = format!("{:x}", Sha256::digest(old_token.as_bytes()));
        fs::write(&path, &old_digest).unwrap();
        let first = format!("nomad_reader={old_token}");
        let mut pairing = Pairing::load(Some(path.clone())).unwrap();
        assert!(pairing.allows(&first));
        assert_eq!(pairing.toggle_replies(), Some(true));
        let code = pairing.issue();
        assert!(!pairing.replies_enabled());
        let token = pairing.exchange(&code).unwrap().unwrap();
        let second = format!("nomad_reader={token}");
        assert!(pairing.exchange(&code).unwrap().is_none());
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains(&old_token) && !saved.contains(&token));
        assert!(pairing.allows(&first) && pairing.allows(&second));
        let mut restarted = Pairing::load(Some(path.clone())).unwrap();
        assert!(restarted.allows(&first) && restarted.allows(&second));
        assert!(!restarted.replies_enabled());
        restarted.revoke().unwrap();
        for state in [restarted, Pairing::load(Some(path)).unwrap()] {
            assert!(!state.allows(&first) && !state.allows(&second));
            assert!(!state.replies_enabled());
        }
    }
}
