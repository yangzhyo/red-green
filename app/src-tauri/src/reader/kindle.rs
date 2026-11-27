//! Server-rendered, read-only pages for the Paperwhite browser.
use super::{
    model::Message,
    pairing::Pairing,
    server::{device_cookie, escaped, send},
    Source,
};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::{json, Value};
use std::{fmt::Write, io::Read, sync::Mutex};
use tiny_http::{Method, Request};

const HTML: &str = "text/html; charset=utf-8";
// Count Unicode characters, independently of window size or UTF-8 byte length.
const PART_SIZE: usize = 280;

#[derive(Default)]
pub(super) struct Position {
    id: Option<String>,
    part: usize,
    step: i8,
}

impl Position {
    pub fn parse(query: &str) -> Result<Self, u16> {
        let mut pos = Self::default();
        if query.len() > 8192 {
            return Err(400);
        }
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "id" => pos.id = Some(value.into_owned()),
                "part" => pos.part = value.parse().map_err(|_| 400u16)?,
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

    pub fn needs_older(&self, rows: &[Message]) -> bool {
        self.step == -1
            && self.part == 0
            && rows
                .first()
                .is_some_and(|m| Some(&m.id) == self.id.as_ref())
    }
}

fn last_part(text: &str) -> usize {
    text.chars().count().saturating_sub(1) / PART_SIZE
}

pub(super) fn page(
    rows: &[Message],
    pos: &Position,
    older: bool,
    revision: &str,
) -> Result<Value, u16> {
    let Some(last) = rows.len().checked_sub(1) else {
        return if pos.id.is_some() {
            Err(409)
        } else {
            Ok(json!({"revision":revision,"tail":true}))
        };
    };
    let mut index = match &pos.id {
        Some(id) => rows.iter().position(|m| &m.id == id).ok_or(409u16)?,
        None => last,
    };
    let end = last_part(&rows[index].text);
    let mut part = if pos.id.is_some() {
        pos.part.min(end)
    } else {
        end
    };
    match pos.step {
        -1 if part > 0 => part -= 1,
        -1 if index > 0 => {
            index -= 1;
            part = last_part(&rows[index].text);
        }
        1 if part < end => part += 1,
        1 if index < last => {
            index += 1;
            part = 0;
        }
        _ => (),
    }
    let m = &rows[index];
    let mut chars = m.text.chars().skip(part * PART_SIZE);
    let text: String = chars.by_ref().take(PART_SIZE).collect();
    let next = index < last || chars.next().is_some();
    Ok(
        json!({"id":m.id,"role":m.role,"text":text,"part":part,"prev":older || index>0 || part>0,
        "next":next,"tail":!next,"revision":revision}),
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
            r#"<p class="row"><a href="/kindle/read/{}">{}</a><br><small>{} · {} · {}</small></p>"#,
            utf8_percent_encode(key, NON_ALPHANUMERIC),
            escaped(title),
            escaped(entry["project"].as_str().unwrap_or("")),
            escaped(entry["source"].as_str().unwrap_or("")),
            escaped(key)
        )
        .unwrap();
    }
    html.push_str(r#"<p><a href="/kindle">刷新列表</a></p>"#);
    Ok(document("当前会话", &html, false))
}

fn thread(page: &Value, path: &str) -> String {
    let path = escaped(path);
    let mut html = String::new();
    if let Some(id) = page["id"].as_str() {
        let speaker = if page["role"] == "user" {
            "你"
        } else {
            "助手"
        };
        write!(
            html,
            "<h1>{speaker}</h1><pre>{}</pre><nav>",
            escaped(page["text"].as_str().unwrap_or(""))
        )
        .unwrap();
        let part = page["part"].as_u64().unwrap_or(0);
        for (step, label) in [("prev", "上一屏"), ("next", "下一屏")] {
            if page[step] == true {
                let query = form_urlencoded::Serializer::new(String::new())
                    .append_pair("id", id)
                    .append_pair("part", &part.to_string())
                    .append_pair("step", step)
                    .finish();
                write!(
                    html,
                    r#"<a rel="{step}" href="{path}?{}">{label}</a> "#,
                    escaped(&query)
                )
                .unwrap();
            }
        }
        html.push_str("</nav>");
    } else {
        html.push_str("<p>等待会话正文</p>");
    }
    write!(
        html,
        r#"<p><a id="latest" href="{path}">查看最新</a> · <a href="/kindle">返回列表</a></p>"#
    )
    .unwrap();
    html
}

fn thread_response(
    source: &mut impl Source,
    encoded_key: &str,
    path: &str,
    query: &str,
    polling: Option<&str>,
) -> Result<(u16, &'static str, String), u16> {
    let key = percent_encoding::percent_decode_str(encoded_key)
        .decode_utf8()
        .map_err(|_| 400u16)?;
    let page = source.kindle_messages(&key, query)?;
    let revision = page["revision"].as_str().ok_or(503u16)?;
    if let Some(old) = polling {
        if old == revision {
            return Ok((204, "application/json", String::new()));
        }
        let update = json!({"revision": revision, "html": thread(&page, path)});
        return Ok((200, "application/json", update.to_string()));
    }
    let content = format!(
        r#"<main id="reader" data-revision="{}" data-tail="{}">{}</main><p id="notice" role="status"></p>"#,
        escaped(revision),
        page["tail"],
        thread(&page, path)
    );
    let title = page["title"].as_str().unwrap_or("当前会话");
    Ok((200, HTML, document(title, &content, true)))
}

pub(super) fn handle(
    mut req: Request,
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
    if req.method() != &Method::Get {
        return send(req, 405, HTML, String::new(), None, None);
    }
    let asset = match path {
        "/kindle/reader.css" => Some(("text/css", include_str!("../../../ui/kindle.css"))),
        "/kindle/update.js" => Some(("text/javascript", include_str!("../../../ui/kindle.js"))),
        _ => None,
    };
    if let Some((mime, data)) = asset {
        return send(req, 200, mime, data.into(), None, None);
    }
    let polling = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("X-Kindle-Since"))
        .map(|h| h.value.as_str().to_owned());
    let result = if !pairing.lock().unwrap().allows(cookie) {
        Err(401)
    } else if path == "/kindle" {
        listing(source.sessions()).map(|s| (200, HTML, s))
    } else if let Some(key) = path
        .strip_prefix("/kindle/read/")
        .filter(|s| !s.is_empty() && !s.contains('/'))
    {
        thread_response(source, key, path, query, polling.as_deref())
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
