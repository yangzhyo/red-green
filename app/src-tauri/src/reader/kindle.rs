//! Server-rendered pages for the Paperwhite browser.
mod reply;

use super::{
    model::Message,
    pairing::Pairing,
    server::{device_cookie, escaped, send, session_state_label},
    Source,
};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::{json, Value};
use std::{fmt::Write, io::Read, sync::Mutex};
use tiny_http::{Method, Request};

const HTML: &str = "text/html; charset=utf-8";
#[derive(Default)]
pub(super) struct Position {
    id: Option<String>,
    step: i8,
    legacy_part: Option<usize>,
    since: Option<String>,
    status_only: bool,
}

impl Position {
    pub fn parse(query: &str) -> Result<Self, u16> {
        let mut pos = Self::default();
        if query.len() > 8192 {
            return Err(400);
        }
        if query == "latest=1" {
            return Ok(pos);
        }
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "id" => pos.id = Some(value.into_owned()),
                "part" => {
                    pos.legacy_part = Some(value.parse().map_err(|_| 400u16)?);
                }
                "step" => {
                    pos.step = match value.as_ref() {
                        "prev" => -1,
                        "next" => 1,
                        _ => return Err(400),
                    }
                }
                _ => return Err(400),
            }
        }
        if !query.is_empty() && pos.id.is_none() {
            return Err(400);
        }
        Ok(pos)
    }

    pub fn poll(since: &str, status_only: bool) -> Self {
        Self {
            since: Some(since.to_owned()),
            status_only,
            ..Self::default()
        }
    }

    pub fn needs_older(&self, rows: &[Message]) -> bool {
        if let Some(part) = self.legacy_part {
            return part == 0
                && self.step == -1
                && rows
                    .first()
                    .is_some_and(|row| Some(&row.id) == self.id.as_ref());
        }
        self.step == -1
            && rows
                .iter()
                .enumerate()
                .take_while(|(i, row)| *i == 0 || !row.starts_turn)
                .any(|(_, row)| Some(&row.id) == self.id.as_ref())
    }
}

// Older bookmarks describe a step from a 280-character fragment. Only the
// last fragment of the last message in a turn can advance to another turn.
fn legacy_last_part(text: &str) -> usize {
    let document = dom_query::Document::fragment(super::markdown::render(text));
    let length: usize = document
        .html_root()
        .descendants_it()
        .filter(|n| n.first_child().is_none())
        .map(|n| {
            if !n.is_text() {
                return 1;
            }
            let text = n.text();
            if text.as_ref() == "\n"
                && n.parent().and_then(|p| p.node_name()).as_deref() != Some("code")
            {
                0
            } else {
                text.chars().count()
            }
        })
        .sum();
    length.saturating_sub(1) / 280
}

pub(super) fn page(
    rows: &[Message],
    pos: &Position,
    older: bool,
    revision: &str,
) -> Result<Value, u16> {
    if pos.since.as_deref() == Some(revision) {
        return Ok(json!({"revision":revision}));
    }
    if rows.is_empty() {
        return if pos.id.is_some() {
            Err(409)
        } else {
            Ok(json!({"revision":revision,"tail":true}))
        };
    }
    let index = match &pos.id {
        Some(id) => rows.iter().position(|m| &m.id == id).ok_or(409u16)?,
        None => rows.len() - 1,
    };
    let turn_start = |index| {
        rows[..=index]
            .iter()
            .rposition(|m| m.starts_turn)
            .unwrap_or(0)
    };
    let turn_end = |start: usize| {
        rows[start + 1..]
            .iter()
            .position(|m| m.starts_turn)
            .map_or(rows.len(), |offset| start + 1 + offset)
    };
    let mut start = turn_start(index);
    let end = turn_end(start);
    let step = match (pos.legacy_part, pos.step) {
        (Some(part), -1) if index != start || part > 0 => 0,
        (Some(part), 1)
            if index + 1 != end
                || end == rows.len()
                || part < legacy_last_part(&rows[index].text) =>
        {
            0
        }
        _ => pos.step,
    };
    match step {
        -1 if start > 0 => start = turn_start(start - 1),
        1 => {
            if end < rows.len() {
                start = end;
            }
        }
        _ => (),
    }
    let end = turn_end(start);
    if pos.status_only {
        return Ok(json!({"revision":revision, "id":rows[start].id}));
    }
    let messages: Vec<_> = rows[start..end]
        .iter()
        .map(|m| json!({"role":m.role,"text":m.text}))
        .collect();
    Ok(
        json!({"id":rows[start].id, "messages":messages, "prev":older || start > 0,
        "next":end < rows.len(), "tail":end == rows.len(), "revision":revision}),
    )
}

fn document(title: &str, content: &str, live: bool) -> String {
    let script = if live {
        r#"<script src="/kindle/update.js"></script>"#
    } else {
        ""
    };
    format!(
        r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><link rel="stylesheet" href="/kindle/reader.css"></head><body>{content}{script}</body></html>"#,
        escaped(title)
    )
}

fn pair_form(request: Request, pairing: &Mutex<Pairing>, status: u16, message: &str) {
    let nonce = pairing.lock().unwrap().new_form();
    let content = format!(
        r#"<h1>Kindle 阅读</h1><p>{message}</p><form action="/kindle/pair" method="post"><label for="code">输入 Mac 上的六位配对码</label><p><input id="code" type="text" name="code" size="7" maxlength="6" required><input type="hidden" name="form" value="{nonce}"> <button type="submit">配对</button></p></form>"#
    );
    send(
        request,
        status,
        HTML,
        document("设备配对", &content, false),
        Some(format!(
            "kindle_form={nonce}; HttpOnly; SameSite=Strict; Path=/kindle; Max-Age=600"
        )),
        None,
    );
}

fn submit(request: &mut Request, pairing: &Mutex<Pairing>, cookies: &str) -> Result<String, u16> {
    if !request.body_length().is_some_and(|n| n <= 160)
        || !request.headers().iter().any(|h| {
            h.field.equiv("Content-Type")
                && h.value.as_str().split(';').next() == Some("application/x-www-form-urlencoded")
        })
    {
        return Err(400);
    }
    let mut data = Vec::new();
    request
        .as_reader()
        .take(161)
        .read_to_end(&mut data)
        .map_err(|_| 400u16)?;
    let mut code = None;
    let mut form = None;
    for (key, value) in form_urlencoded::parse(&data) {
        match key.as_ref() {
            "code" => code = Some(value),
            "form" => form = Some(value),
            _ => return Err(400),
        }
    }
    let nonce = form.ok_or(403u16)?;
    let code = code
        .filter(|v| v.len() == 6 && v.bytes().all(|b| b.is_ascii_digit()))
        .ok_or(403u16)?;
    let mut device = pairing.lock().unwrap();
    if !cookies
        .split(';')
        .any(|s| s.trim().strip_prefix("kindle_form=") == Some(nonce.as_ref()))
        || !device.consume_form(&nonce)
    {
        return Err(403);
    }
    device.exchange(&code).map_err(|_| 503u16)?.ok_or(403)
}

fn listing(entries: Value) -> Result<String, u16> {
    let rows = entries.as_array().ok_or(503u16)?;
    let mut html = String::from("<h1>当前会话</h1>");
    if rows.is_empty() {
        html.push_str("<p>等待可读会话</p>");
    }
    for entry in rows {
        let key = entry["key"].as_str().ok_or(503u16)?;
        let title = entry["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("未命名会话");
        write!(
            html,
            r#"<p class="row"><a href="/kindle/read/{}">{}</a><br><small>{} · {}</small></p>"#,
            utf8_percent_encode(key, NON_ALPHANUMERIC),
            escaped(title),
            escaped(entry["project"].as_str().unwrap_or("")),
            escaped(entry["source"].as_str().unwrap_or(""))
        )
        .unwrap();
    }
    html.push_str(r#"<p><a href="/kindle">刷新列表</a></p>"#);
    Ok(document("当前会话", &html, false))
}

fn thread(page: &Value, path: &str, reply: Option<&str>, state: &str, label: &str) -> String {
    let path = escaped(path);
    let mut html = String::new();
    if let Some(messages) = page["messages"].as_array() {
        for message in messages {
            let speaker = if message["role"] == "user" {
                "你"
            } else {
                "助手"
            };
            let body = super::markdown::render(message["text"].as_str().unwrap_or(""));
            write!(
                html,
                r#"<h1>{speaker}</h1><div class="markdown">{body}</div>"#
            )
            .unwrap();
        }
    } else {
        html.push_str("<p>等待会话正文</p>");
    }
    write!(html, r#"<nav id="reader-nav" aria-label="阅读导航"><span id="session-state" role="status" data-state="{state}">{label}</span><a id="latest" href="{path}?latest=1">查看最新</a> <a href="/kindle">返回列表</a> "#).unwrap();
    if let Some(url) = reply {
        write!(
            html,
            r#"<a id="kindle-reply" href="{}">回复</a> "#,
            escaped(url)
        )
        .unwrap();
    }
    for (step, label) in [("prev", "上一屏"), ("next", "下一屏")] {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("id", page["id"].as_str().unwrap_or(""))
            .append_pair("step", step)
            .finish();
        let url = format!("{path}?{}", escaped(&query));
        if page[step] == true {
            write!(html, r#"<a rel="{step}" href="{url}">{label}</a> "#).unwrap();
        } else {
            write!(
                html,
                r#"<a rel="{step}" aria-disabled="true" data-href="{url}">{label}</a> "#
            )
            .unwrap();
        }
    }
    html.push_str("</nav>");
    html
}

struct Poll<'a> {
    since: &'a str,
    state: Option<&'a str>,
    status_only: bool,
    had_reply: bool,
}

impl<'a> Poll<'a> {
    fn from_request(req: &'a Request) -> Option<Self> {
        let header = |name: &'static str| {
            req.headers()
                .iter()
                .find(|h| h.field.equiv(name))
                .map(|h| h.value.as_str())
        };
        Some(Self {
            since: header("X-Kindle-Since")?,
            state: header("X-Kindle-State"),
            status_only: header("X-Kindle-Status-Only") == Some("1"),
            had_reply: header("X-Kindle-Reply") == Some("1"),
        })
    }
}

fn thread_response(
    source: &mut impl Source,
    encoded_key: &str,
    path: &str,
    query: &str,
    polling: Option<&Poll>,
    replies_enabled: bool,
) -> Result<(u16, &'static str, String), u16> {
    let key = percent_encoding::percent_decode_str(encoded_key)
        .decode_utf8()
        .map_err(|_| 400u16)?;
    let reply = (replies_enabled && key.starts_with("codex-"))
        .then(|| format!("/kindle/reply/{encoded_key}"));
    let page = match polling {
        Some(poll) => source.kindle_poll(&key, poll.since, poll.status_only)?,
        None => source.kindle_messages(&key, query)?,
    };
    let revision = page["revision"].as_str().ok_or(503u16)?;
    let raw = page["state"].as_str().unwrap_or("");
    let (state, label) = match session_state_label(raw) {
        Some(label) => (raw, label),
        None => ("unknown", "状态未知"),
    };
    if let Some(poll) = polling {
        let same_body = poll.since == revision;
        let state_changed = poll.state.is_some_and(|previous| previous != state);
        if same_body && !state_changed && poll.had_reply == reply.is_some() {
            return Ok((204, "application/json", String::new()));
        }
        let mut update = json!({"reply":reply, "state":state, "stateLabel":label});
        if same_body {
            // Older open tabs understand this flag for updates without a body revision.
            update["permissionOnly"] = true.into();
        } else {
            update["revision"] = revision.into();
            update["turn"] = page["id"].clone();
            if !poll.status_only {
                update["html"] = thread(&page, path, reply.as_deref(), state, label).into();
            }
        }
        return Ok((200, "application/json", update.to_string()));
    }
    let content = format!(
        r#"<main id="reader" data-revision="{}" data-tail="{}" data-turn="{}">{}</main><p id="notice" role="status"></p>"#,
        escaped(revision),
        page["tail"],
        escaped(page["id"].as_str().unwrap_or("")),
        thread(&page, path, reply.as_deref(), state, label)
    );
    let title = page["title"].as_str().unwrap_or("当前会话");
    Ok((200, HTML, document(title, &content, true)))
}

pub(super) fn handle(
    mut req: Request,
    address: std::net::SocketAddr,
    path: &str,
    query: &str,
    pairing: &Mutex<Pairing>,
    cookie: &str,
    source: &mut impl Source,
) {
    if req.method() == &Method::Post && path == "/kindle/pair" {
        return match submit(&mut req, pairing, cookie) {
            Ok(token) => send(
                req,
                303,
                HTML,
                String::new(),
                Some(device_cookie(&token)),
                Some("/kindle"),
            ),
            Err(status) => pair_form(
                req,
                pairing,
                status,
                if status == 503 {
                    "暂时无法保存配对，请重试。"
                } else {
                    "配对码或表单已失效，请重新输入。"
                },
            ),
        };
    }
    if let Some(key) = path
        .strip_prefix("/kindle/reply/")
        .filter(|s| !s.is_empty() && !s.contains('/'))
    {
        return reply::handle(req, address, key, pairing, cookie, source);
    }
    if req.method() != &Method::Get {
        return send(req, 405, HTML, String::new(), None, None);
    }
    let asset = match path {
        "/kindle/reader.css" => Some(("text/css", include_str!("../../../ui/kindle.css"))),
        "/kindle/update.js" => Some((
            "text/javascript",
            concat!(
                include_str!("../../../ui/reading-gestures.js"),
                "\n",
                include_str!("../../../ui/kindle.js")
            ),
        )),
        _ => None,
    };
    if let Some((mime, data)) = asset {
        return send(req, 200, mime, data.into(), None, None);
    }
    let polling = Poll::from_request(&req);
    let result = if !pairing.lock().unwrap().allows(cookie) {
        Err(401)
    } else if path == "/kindle" {
        listing(source.sessions()).map(|s| (200, HTML, s))
    } else if let Some(key) = path
        .strip_prefix("/kindle/read/")
        .filter(|s| !s.is_empty() && !s.contains('/'))
    {
        let replies_enabled = pairing.lock().unwrap().replies_enabled();
        thread_response(source, key, path, query, polling.as_ref(), replies_enabled)
    } else {
        Err(404)
    };
    if !pairing.lock().unwrap().allows(cookie) || matches!(result, Err(401)) {
        if polling.is_some() {
            return send(req, 401, "application/json", "{}".into(), None, None);
        }
        return pair_form(req, pairing, 200, "");
    }
    match result {
        Ok((status, mime, body)) => send(req, status, mime, body, None, None),
        Err(status) => {
            let reason = match status {
                409 => "阅读位置已失效",
                410 => "会话已结束",
                404 => "找不到会话",
                _ => "暂时无法读取会话",
            };
            let body = format!(
                r#"<h1>{reason}</h1><p><a href="{}">重新打开</a> · <a href="/kindle">返回列表</a></p>"#,
                escaped(path)
            );
            send(
                req,
                status,
                HTML,
                document(reason, &body, false),
                None,
                None,
            )
        }
    }
}
