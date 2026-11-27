use super::{kindle, pairing::Pairing, Library};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
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
    fn current_entry(&mut self, key: &str) -> Result<Value, u16> {
        self.sessions()
            .as_array_mut()
            .and_then(|rows| rows.iter_mut().find(|entry| entry["key"] == key))
            .map(Value::take)
            .ok_or(404)
    }
    fn kindle_messages(&mut self, _key: &str, _query: &str) -> Result<Value, u16> {
        Err(501)
    }
    fn kindle_poll(&mut self, key: &str, _since: &str, _status_only: bool) -> Result<Value, u16> {
        self.kindle_messages(key, "")
    }
    fn history_entry(&mut self, _key: &str) -> Result<Value, u16> {
        Err(404)
    }
    fn history(&mut self, _cursor: Option<&str>) -> Result<Value, u16> {
        Ok(json!({"entries": [], "nextCursor": null, "generation":"demo"}))
    }
    fn history_status(&mut self, generation: &str) -> Result<Value, u16> {
        (generation == "demo").then(|| json!({})).ok_or(409)
    }
    fn history_messages(&mut self, _key: &str, _query: &str) -> Result<Value, u16> {
        Err(404)
    }
    fn resume(
        &mut self,
        _key: &str,
        _admit: impl FnOnce() -> Result<(), u16>,
    ) -> Result<Value, u16> {
        Err(405)
    }
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
    fn kindle_messages(&mut self, key: &str, query: &str) -> Result<Value, u16> {
        let pos = kindle::Position::parse(query)?;
        let lib = self.lock().unwrap();
        lib.sessions
            .get(key)
            .ok_or(404u16)?
            .kindle_page(&pos, false)
    }
    fn kindle_poll(&mut self, key: &str, since: &str, status_only: bool) -> Result<Value, u16> {
        self.lock()
            .unwrap()
            .sessions
            .get(key)
            .ok_or(404u16)?
            .kindle_page(&kindle::Position::poll(since, status_only), false)
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
    send(request, status, mime, body, cookie, None);
}

pub(super) fn device_cookie(token: &str) -> String {
    format!("nomad_reader={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000")
}

pub(super) fn send(
    request: Request,
    status: u16,
    mime: &str,
    body: String,
    cookie: Option<String>,
    location: Option<&str>,
) {
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
    if let Some(path) = location {
        response.add_header(header("Location", path));
    }
    let _ = request.respond(response);
}
const JSON: &str = "application/json";
const HTML: &str = "text/html; charset=utf-8";
const READER_HTML: &str = include_str!("../../../ui/reader.html");

#[derive(Deserialize, Serialize)]
struct DocumentEntry {
    key: String,
    title: Option<String>,
    project: String,
    source: String,
    state: Option<String>,
}

impl DocumentEntry {
    fn read(
        pairing: &Mutex<Pairing>,
        cookie: &str,
        source: &mut impl Source,
        key: &str,
        history: bool,
    ) -> Result<Self, u16> {
        if !pairing.lock().unwrap().allows(cookie) {
            return Err(401);
        }
        let result = if history {
            source.history_entry(key)
        } else {
            source.current_entry(key)
        };
        // 配对可能在取标题期间被撤销，与正文使用同样的二次校验。
        if !pairing.lock().unwrap().allows(cookie) {
            return Err(401);
        }
        let entry: Self = serde_json::from_value(result?).map_err(|_| 503u16)?;
        (entry.key == key).then_some(entry).ok_or(404)
    }

    fn page_title(&self, history: bool) -> String {
        let label = match self.state.as_deref() {
            Some("running") => "运行中",
            Some("awaiting") => "待确认",
            Some("your_turn") => "轮到你",
            Some("completed") => "已完成",
            Some("aborted") => "异常中止",
            Some("idle") => "空闲",
            _ if history => "只读",
            _ => "",
        };
        let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let project = normalize(&self.project);
        let name = normalize(self.title.as_deref().unwrap_or(""));
        let name = if name.is_empty() {
            "未命名会话"
        } else {
            &name
        };
        [
            normalize(&self.source).as_str(),
            label,
            &project,
            if name == project { "" } else { name },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

pub(super) fn escaped(text: &str) -> String {
    let mut html = String::new();
    pulldown_cmark_escape::escape_html(&mut html, text).expect("writing to a string");
    html
}

fn document(title: &str, body_attributes: &str) -> String {
    READER_HTML
        .replacen(
            "<title>red-green · 当前会话</title>",
            &format!("<title>{}</title>", escaped(title)),
            1,
        )
        .replacen(
            r#"<body data-page="list">"#,
            &format!("<body {body_attributes}>"),
            1,
        )
}

fn thread_document(
    request: Request,
    pairing: &Mutex<Pairing>,
    cookie: &str,
    source: &mut impl Source,
    key: &str,
    history: bool,
) {
    let entry = DocumentEntry::read(pairing, cookie, source, key, history);
    let wants_json = request
        .headers()
        .iter()
        .any(|h| h.field.equiv("Accept") && h.value.as_str() == JSON);
    if wants_json {
        return match entry {
            Ok(entry) => reply(request, 200, JSON, json!(entry).to_string(), None),
            Err(status) => reply(request, status, JSON, "{}".into(), None),
        };
    }
    let html = match entry {
        Ok(entry) => document(
            &entry.page_title(history),
            &format!(
                r#"data-page="thread" data-entry="{}""#,
                escaped(&json!(entry).to_string())
            ),
        ),
        Err(status) => document(
            if status == 401 {
                "red-green · 设备配对"
            } else {
                "red-green · 会话"
            },
            &format!(r#"data-page="thread" data-reader-error="{status}""#),
        ),
    };
    reply(request, 200, HTML, html, None);
}

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
            "/" => Some((HTML, READER_HTML)),
            "/pair" => {
                return reply(
                    request,
                    200,
                    HTML,
                    document("red-green · 设备配对", r#"data-page="pair""#),
                    None,
                )
            }
            "/archive" => {
                return reply(
                    request,
                    200,
                    HTML,
                    document("red-green · 历史会话", r#"data-page="history""#),
                    None,
                )
            }
            "/reader.js" => Some(("text/javascript", include_str!("../../../ui/reader.js"))),
            "/reading-gestures.js" => Some((
                "text/javascript",
                include_str!("../../../ui/reading-gestures.js"),
            )),
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
            Ok(Some(Some(token))) => {
                reply(request, 200, JSON, "{}".into(), Some(device_cookie(&token)))
            }
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
    if path == "/kindle" || path.starts_with("/kindle/") {
        return kindle::handle(request, address, path, query, pairing, &cookie, source);
    }
    if request.method() == &Method::Get {
        if let Some(key) = route_key(path, "/read/current/", "") {
            return thread_document(request, pairing, &cookie, source, key, false);
        }
        if let Some(key) = route_key(path, "/read/history/", "") {
            return thread_document(request, pairing, &cookie, source, key, true);
        }
    }
    if !pairing.lock().unwrap().allows(&cookie) {
        return reply(request, 401, JSON, "{}".into(), None);
    }
    let live_key = route_key(path, "/sessions/", "/messages");
    let history_key = route_key(path, "/history/", "/messages");
    if request.method() == &Method::Post {
        let result = if let Some(key) = live_key {
            write_action(
                &mut request,
                address,
                pairing,
                &cookie,
                source,
                Action::Reply(key),
            )
        } else if let Some(key) = route_key(path, "/history/", "/resume") {
            write_action(
                &mut request,
                address,
                pairing,
                &cookie,
                source,
                Action::Resume(key),
            )
        } else {
            Err(405)
        };
        return match result {
            Ok(data) => reply(request, 200, JSON, data.to_string(), None),
            Err(code) => reply(request, code, JSON, "{}".into(), None),
        };
    }
    if request.method() != &Method::Get {
        return reply(request, 405, JSON, "{}".into(), None);
    }
    let mut data = match (path, live_key, history_key) {
        ("/sessions", _, _) => Ok(source.sessions()),
        ("/history", _, _) if query.is_empty() => source.history(None),
        ("/history", _, _) => {
            query_token(query, "cursor=", 8192).and_then(|v| source.history(Some(&v)))
        }
        ("/history/status", _, _) => {
            query_token(query, "generation=", 128).and_then(|v| source.history_status(&v))
        }
        (_, Some(key), _) => source.messages(key, query),
        (_, _, Some(key)) => source.history_messages(key, query),
        _ => Err(404u16),
    };
    // A slow source read might have overlapped a revocation on the Mac.
    {
        let mut device = pairing.lock().unwrap();
        if !device.allows(&cookie) {
            return reply(request, 401, JSON, "{}".into(), None);
        }
        if let Ok(page) = &mut data {
            if let Some(key) = live_key.filter(|k| k.starts_with("codex-")) {
                if let Some(ticket) = device.reply_ticket(key) {
                    page["replyTicket"] = ticket.into();
                }
            } else if let Some(key) = history_key {
                if let Some(ticket) = device.reply_ticket(&format!("resume/{key}")) {
                    page["resumeTicket"] = ticket.into();
                }
            }
        }
    }
    match data {
        Ok(value) => reply(request, 200, JSON, value.to_string(), None),
        Err(status) => reply(request, status, JSON, "{}".into(), None),
    }
}

fn route_key<'a>(path: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    path.strip_prefix(prefix)?
        .strip_suffix(suffix)
        .filter(|s| !s.is_empty() && !s.contains('/'))
}

fn query_token<'a>(
    query: &'a str,
    prefix: &str,
    limit: usize,
) -> Result<std::borrow::Cow<'a, str>, u16> {
    let encoded = query
        .strip_prefix(prefix)
        .filter(|v| !v.is_empty() && v.len() <= limit && !v.contains('&'))
        .ok_or(400u16)?;
    percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .map_err(|_| 400u16)
}

enum Action<'a> {
    Reply(&'a str),
    Resume(&'a str),
}

fn write_action(
    request: &mut Request,
    address: SocketAddr,
    pairing: &Mutex<Pairing>,
    cookie: &str,
    source: &mut impl Source,
    action: Action<'_>,
) -> Result<Value, u16> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Submission {
        ticket: String,
        text: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Resume {
        ticket: String,
    }

    let key = match action {
        Action::Reply(key) | Action::Resume(key) => key,
    };
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

    match action {
        Action::Reply(_) => {
            let data: Submission = read_json(request, 64 * 1024)?;
            submit_reply(source, pairing, cookie, key, &data.ticket, &data.text)?;
            Ok(json!({}))
        }
        Action::Resume(_) => {
            let data: Resume = read_json(request, 256)?;
            let scope = format!("resume/{key}");
            let admit = action_guard(pairing, cookie, &scope, &data.ticket)?;
            source.resume(key, admit)
        }
    }
}

pub(super) fn submit_reply(
    source: &mut impl Source,
    pairing: &Mutex<Pairing>,
    cookie: &str,
    key: &str,
    ticket: &str,
    text: &str,
) -> Result<(), u16> {
    if !key.starts_with("codex-") {
        return Err(405);
    }
    if text.trim().is_empty() || text.chars().count() > 8192 {
        return Err(422);
    }
    source.send_reply(key, text, action_guard(pairing, cookie, key, ticket)?)
}

fn action_guard<'a>(
    pairing: &'a Mutex<Pairing>,
    cookie: &'a str,
    scope: &'a str,
    ticket: &'a str,
) -> Result<impl FnOnce() -> Result<(), u16> + 'a, u16> {
    pairing.lock().unwrap().check_ticket(scope, ticket)?;
    Ok(move || {
        let mut device = pairing.lock().unwrap();
        if !device.allows(cookie) {
            return Err(401);
        }
        device.take_ticket(scope, ticket)
    })
}

fn read_json<T: DeserializeOwned>(request: &mut Request, limit: usize) -> Result<T, u16> {
    if !request.body_length().is_some_and(|n| n <= limit) {
        return Err(413);
    }
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| 400u16)?;
    serde_json::from_slice(&bytes).map_err(|_| 400u16)
}
