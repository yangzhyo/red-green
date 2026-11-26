use super::{pairing::Pairing, Library};
use serde::Deserialize;
use std::{
    io::{self, Read},
    net::{SocketAddr, TcpListener},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub trait Source: Send + 'static {
    fn sessions(&mut self) -> serde_json::Value;
    fn send_reply(
        &mut self,
        _key: &str,
        _text: &str,
        _admit: impl FnOnce() -> Result<(), u16>,
    ) -> Result<(), u16> {
        Err(405)
    }
    fn messages(&mut self, key: &str, query: &str) -> Result<serde_json::Value, u16>;
}

impl Source for Arc<Mutex<Library>> {
    fn sessions(&mut self) -> serde_json::Value {
        self.lock().unwrap().list()
    }
    fn messages(&mut self, key: &str, query: &str) -> Result<serde_json::Value, u16> {
        self.lock().unwrap().messages(key, query).ok_or(404)
    }
}

pub struct Reader {
    address: SocketAddr,
    server: Arc<Server>,
    running: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    pairing: Arc<Mutex<Pairing>>,
}

impl Reader {
    pub fn start(address: SocketAddr, source: impl Source) -> io::Result<Self> {
        Self::with_store(address, source, None)
    }
    pub fn with_store(
        address: SocketAddr,
        mut source: impl Source,
        store: Option<std::path::PathBuf>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        let address = listener.local_addr()?;
        let server = Arc::new(Server::from_listener(listener, None).map_err(io::Error::other)?);
        let pairing = Arc::new(Mutex::new(Pairing::load(store)?));
        let running = Arc::new(AtomicBool::new(true));
        let worker = {
            let server = server.clone();
            let pairing = pairing.clone();
            let running = running.clone();
            thread::spawn(move || {
                while running.load(Ordering::Acquire) {
                    let Ok(request) = server.recv() else { break };
                    handle(request, address, &pairing, &mut source);
                }
            })
        };
        Ok(Self {
            address,
            server,
            running,
            pairing,
            worker: Some(worker),
        })
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn new_code(&self) -> String {
        self.pairing.lock().unwrap().issue()
    }
    pub fn toggle_replies(&self) -> Option<bool> {
        self.pairing.lock().unwrap().toggle_replies()
    }
    pub fn revoke(&self) -> io::Result<()> {
        self.pairing.lock().unwrap().revoke()
    }
    pub fn suspend(&self) {
        *self.pairing.lock().unwrap() = Pairing::default();
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).expect("valid response header")
}
fn reply(request: Request, status: u16, mime: &str, body: String, cookie: Option<String>) {
    let mut response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", mime))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("X-Content-Type-Options", "nosniff"))
        .with_header(header(
            "Content-Security-Policy",
            "default-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'",
        ));
    if let Some(value) = cookie {
        response.add_header(header("Set-Cookie", &value));
    }
    let _ = request.respond(response);
}
const JSON: &str = "application/json";
fn handle(
    mut request: Request,
    address: SocketAddr,
    pairing: &Mutex<Pairing>,
    source: &mut impl Source,
) {
    let url = request.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    if request.method() == &Method::Get {
        let asset = match path {
            "/" => Some((
                "text/html; charset=utf-8",
                include_str!("../../../ui/reader.html"),
            )),
            "/reader.js" => Some(("text/javascript", include_str!("../../../ui/reader.js"))),
            "/reader-model.js" => Some((
                "text/javascript",
                include_str!("../../../ui/reader-model.js"),
            )),
            "/skins.js" => Some(("text/javascript", include_str!("../../../ui/skins.js"))),
            "/sprites.js" => Some(("text/javascript", include_str!("../../../ui/sprites.js"))),
            "/reader.css" => Some(("text/css", include_str!("../../../ui/reader.css"))),
            _ => None,
        };
        if let Some((mime, body)) = asset {
            return reply(request, 200, mime, body.into(), None);
        }
    }
    if request.method() == &Method::Post && path == "/pair" {
        #[derive(Deserialize)]
        struct Attempt {
            code: String,
        }
        // Only accept the browser's same-origin JSON request with a bounded body.
        let valid = request
            .headers()
            .iter()
            .any(|h| h.field.equiv("Content-Type") && h.value.as_str() == JSON)
            && request.body_length().is_some_and(|n| n <= 128);
        let mut bytes = Vec::new();
        let token = if valid
            && request
                .as_reader()
                .take(129)
                .read_to_end(&mut bytes)
                .is_ok()
        {
            serde_json::from_slice::<Attempt>(&bytes)
                .ok()
                .map(|v| pairing.lock().unwrap().exchange(&v.code))
                .transpose()
        } else {
            Ok(None)
        };
        return match token {
            Ok(Some(Some(token))) => reply(
                request,
                200,
                JSON,
                "{}".into(),
                Some(format!(
                    "nomad_reader={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000"
                )),
            ),
            Err(_) => reply(request, 503, JSON, "{}".into(), None),
            _ => reply(request, 403, JSON, "{}".into(), None),
        };
    }
    let cookie = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Cookie"))
        .map(|h| h.value.as_str())
        .unwrap_or("")
        .to_owned();
    if !pairing.lock().unwrap().allows(&cookie) {
        return reply(request, 401, JSON, "{}".into(), None);
    }
    let key = path
        .strip_prefix("/sessions/")
        .and_then(|p| p.strip_suffix("/messages"));
    if request.method() == &Method::Post {
        let result = key
            .ok_or(405u16)
            .and_then(|key| send_reply(&mut request, address, pairing, &cookie, source, key));
        return reply(
            request,
            result.err().unwrap_or(200),
            JSON,
            "{}".into(),
            None,
        );
    }
    if request.method() != &Method::Get {
        return reply(request, 405, JSON, "{}".into(), None);
    }
    let mut data = match path {
        "/sessions" => Ok(source.sessions()),
        _ => key
            .ok_or(404u16)
            .and_then(|key| source.messages(key, query)),
    };
    // A slow source read might have overlapped a revocation on the Mac.
    {
        let mut device = pairing.lock().unwrap();
        if !device.allows(&cookie) {
            return reply(request, 401, JSON, "{}".into(), None);
        }
        if let (Some(key), Ok(page)) = (key.filter(|k| k.starts_with("codex-")), &mut data) {
            if let Some(ticket) = device.reply_ticket(key) {
                page["replyTicket"] = ticket.into();
            }
        }
    }
    match data {
        Ok(value) => reply(request, 200, JSON, value.to_string(), None),
        Err(status) => reply(request, status, JSON, "{}".into(), None),
    }
}

fn send_reply(
    request: &mut Request,
    address: SocketAddr,
    pairing: &Mutex<Pairing>,
    cookie: &str,
    source: &mut impl Source,
    key: &str,
) -> Result<(), u16> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Submission {
        ticket: String,
        text: String,
    }
    if !key.starts_with("codex-") {
        return Err(405);
    }
    let has_header = |name: &'static str, value: &str| {
        request
            .headers()
            .iter()
            .any(|h| h.field.equiv(name) && h.value.as_str() == value)
    };
    if !has_header("Origin", &format!("http://{address}")) || !has_header("Content-Type", JSON) {
        return Err(403);
    }
    const MAX_BODY: usize = 64 * 1024;
    if !request.body_length().is_some_and(|n| n <= MAX_BODY) {
        return Err(413);
    }
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| 400u16)?;
    let submission: Submission = serde_json::from_slice(&bytes).map_err(|_| 400u16)?;
    if submission.text.trim().is_empty() || submission.text.chars().count() > 8192 {
        return Err(422);
    }
    pairing
        .lock()
        .unwrap()
        .check_ticket(key, &submission.ticket)?;
    source.send_reply(key, &submission.text, || {
        let mut device = pairing.lock().unwrap();
        if !device.allows(cookie) {
            return Err(401);
        }
        device.take_ticket(key, &submission.ticket)
    })
}
