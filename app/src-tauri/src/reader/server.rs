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
    pub fn start(address: SocketAddr, mut source: impl Source) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        let address = listener.local_addr()?;
        let server = Arc::new(Server::from_listener(listener, None).map_err(io::Error::other)?);
        let pairing = Arc::new(Mutex::new(Pairing::default()));
        let running = Arc::new(AtomicBool::new(true));
        let worker = {
            let server = server.clone();
            let pairing = pairing.clone();
            let running = running.clone();
            thread::spawn(move || {
                while running.load(Ordering::Acquire) {
                    let Ok(request) = server.recv() else { break };
                    handle(request, &pairing, &mut source);
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
    pub fn revoke(&self) {
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
fn handle(mut request: Request, pairing: &Mutex<Pairing>, source: &mut impl Source) {
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
                .and_then(|v| pairing.lock().unwrap().exchange(&v.code))
        } else {
            None
        };
        return match token {
            Some(token) => reply(
                request,
                200,
                JSON,
                "{}".into(),
                Some(format!(
                    "nomad_reader={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000"
                )),
            ),
            None => reply(request, 403, JSON, "{}".into(), None),
        };
    }
    let cookie = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Cookie"))
        .map(|h| h.value.as_str())
        .unwrap_or("");
    if !pairing.lock().unwrap().allows(cookie) {
        return reply(request, 401, JSON, "{}".into(), None);
    }
    if request.method() != &Method::Get {
        return reply(request, 405, JSON, "{}".into(), None);
    }
    let data = match path {
        "/sessions" => Ok(source.sessions()),
        _ => path
            .strip_prefix("/sessions/")
            .and_then(|p| p.strip_suffix("/messages"))
            .ok_or(404u16)
            .and_then(|key| source.messages(key, query)),
    };
    // A slow source read might have overlapped a revocation on the Mac.
    if !pairing.lock().unwrap().allows(cookie) {
        return reply(request, 401, JSON, "{}".into(), None);
    }
    match data {
        Ok(value) => reply(request, 200, JSON, value.to_string(), None),
        Err(status) => reply(request, status, JSON, "{}".into(), None),
    }
}
