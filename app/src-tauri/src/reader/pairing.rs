use rand::Rng;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};

#[derive(Default)]
pub(super) struct Pairing {
    reply_tickets: Option<std::collections::HashMap<String, String>>,
    challenge: Option<(String, Instant, u8)>,
    fingerprint: Option<String>,
    store: Option<PathBuf>,
}

impl Pairing {
    pub fn load(store: Option<PathBuf>) -> io::Result<Self> {
        let fingerprint = match store.as_ref().map(fs::read_to_string).transpose() {
            Ok(value) => value,
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        Ok(Self {
            store,
            fingerprint,
            ..Self::default()
        })
    }

    pub fn revoke(&mut self) -> io::Result<()> {
        self.challenge = None;
        self.fingerprint = None;
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
            if let Err(e) = file
                .write_all(digest.as_bytes())
                .and_then(|()| fs::rename(&temp, path))
            {
                let _ = fs::remove_file(temp);
                return Err(e);
            }
        }
        self.fingerprint = Some(digest);
        Ok(())
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
        *attempts -= 1;
        if code != input {
            return Ok(None);
        }
        self.challenge = None;
        let token = format!(
            "{:032x}{:032x}",
            rand::random::<u128>(),
            rand::random::<u128>()
        );
        self.remember(&token)?;
        Ok(Some(token))
    }
    pub fn toggle_replies(&mut self) -> Option<bool> {
        self.fingerprint.as_ref()?;
        if self.reply_tickets.take().is_some() {
            return Some(false);
        }
        // The grant belongs to the device currently paired with this service.
        self.challenge = None;
        self.reply_tickets = Some(Default::default());
        Some(true)
    }

    pub fn reply_ticket(&mut self, key: &str) -> Option<&str> {
        let tickets = self.reply_tickets.as_mut()?;
        Some(
            tickets
                .entry(key.to_owned())
                .or_insert_with(|| format!("{:032x}", rand::random::<u128>())),
        )
    }

    pub fn check_ticket(&self, key: &str, token: &str) -> Result<(), u16> {
        let tickets = self.reply_tickets.as_ref().ok_or(403u16)?;
        if tickets.get(key).is_some_and(|issued| issued == token) {
            Ok(())
        } else {
            Err(412)
        }
    }

    pub fn take_ticket(&mut self, key: &str, token: &str) -> Result<(), u16> {
        self.check_ticket(key, token)?;
        self.reply_tickets.as_mut().unwrap().remove(key);
        Ok(())
    }

    pub fn allows(&self, cookies: &str) -> bool {
        let Some(expected) = self.fingerprint.as_ref() else {
            return false;
        };
        cookies
            .split(';')
            .filter_map(|part| part.trim().strip_prefix("nomad_reader="))
            .any(|value| {
                value.len() == 64 && format!("{:x}", Sha256::digest(value.as_bytes())) == *expected
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn previous_device_survives_restart_until_revoked() {
        let path = std::env::temp_dir().join(format!("rg-pair-{:x}", rand::random::<u64>()));
        let mut first = Pairing::load(Some(path.clone())).unwrap();
        let code = first.issue();
        let token = first.exchange(&code).unwrap().unwrap();
        let cookie = format!("nomad_reader={token}");
        assert_ne!(fs::read_to_string(&path).unwrap(), token);
        let mut restarted = Pairing::load(Some(path.clone())).unwrap();
        assert!(restarted.allows(&cookie));
        restarted.revoke().unwrap();
        assert!(!Pairing::load(Some(path)).unwrap().allows(&cookie));
    }
}
