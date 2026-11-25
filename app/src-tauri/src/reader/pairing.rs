use rand::Rng;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Pairing {
    challenge: Option<(String, Instant, u8)>,
    token: Option<String>,
}

impl Pairing {
    pub fn issue(&mut self) -> String {
        let code = format!("{:06}", rand::rng().random_range(0..1_000_000));
        self.challenge = Some((code.clone(), Instant::now() + Duration::from_secs(600), 5));
        code
    }
    pub fn exchange(&mut self, input: &str) -> Option<String> {
        let (code, deadline, attempts) = self.challenge.as_mut()?;
        if *attempts == 0 || Instant::now() >= *deadline {
            return None;
        }
        *attempts -= 1;
        if code != input {
            return None;
        }
        self.challenge = None;
        let token = format!(
            "{:032x}{:032x}",
            rand::random::<u128>(),
            rand::random::<u128>()
        );
        self.token = Some(token.clone());
        Some(token)
    }
    pub fn allows(&self, cookies: &str) -> bool {
        self.token.as_ref().is_some_and(|token| {
            cookies
                .split(';')
                .any(|part| part.trim().strip_prefix("nomad_reader=") == Some(token.as_str()))
        })
    }
}
