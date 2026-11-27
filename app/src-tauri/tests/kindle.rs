use red_green::reader::{Conversation, Library, Reader};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
};

fn send(reader: &Reader, path: &str, cookie: &str, since: &str, body: &str) -> String {
    send_from(
        reader,
        path,
        cookie,
        since,
        body,
        &format!("Origin: http://{}\r\n", reader.address()),
    )
}

fn send_from(
    reader: &Reader,
    path: &str,
    cookie: &str,
    since: &str,
    body: &str,
    origin_header: &str,
) -> String {
    let mut tcp = TcpStream::connect(reader.address()).unwrap();
    let method = if body.is_empty() { "GET" } else { "POST" };
    let extra = if since.is_empty() {
        String::new()
    } else {
        format!("X-Kindle-Since: {since}\r\n")
    };
    let content_type = if path == "/pair" {
        "application/json"
    } else {
        "application/x-www-form-urlencoded"
    };
    write!(tcp, "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n{origin_header}Cookie: {cookie}\r\n{extra}Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",reader.address(),body.len()).unwrap();
    let mut response = String::new();
    tcp.read_to_string(&mut response).unwrap();
    response
}

fn attr<'a>(html: &'a str, prefix: &str) -> &'a str {
    html.split_once(prefix)
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
}

fn cookie(response: &str) -> &str {
    response
        .lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
}

#[test]
fn paperwhite_html_form_paging_updates_and_revocation() {
    let lib = Arc::new(Mutex::new(Library::default()));
    let reader = Reader::start("127.0.0.1:0".parse().unwrap(), lib.clone()).unwrap();
    let code = reader.new_code();
    let form = send(&reader, "/kindle", "", "", "");
    let nonce = attr(&form, r#"name="form" value=""#);
    let submit = |form: &str, cookie: &str, code: &str| {
        send(
            &reader,
            "/kindle/pair",
            cookie,
            "",
            &format!("form={form}&code={code}"),
        )
    };
    assert!(submit(nonce, "", &code).starts_with("HTTP/1.1 403"));
    let wrong = if code == "000000" { "000001" } else { "000000" };
    let retry = submit(nonce, cookie(&form), wrong);
    assert!(retry.starts_with("HTTP/1.1 403"));
    assert!(submit(nonce, cookie(&form), &code).starts_with("HTTP/1.1 403"));
    let result = submit(
        attr(&retry, r#"name="form" value=""#),
        cookie(&retry),
        &code,
    );
    assert!(result.starts_with("HTTP/1.1 303") && result.contains("Location: /kindle"));
    let device = cookie(&result);
    let get = |path: &str| send(&reader, path, device, "", "");
    assert!(get("/kindle").contains("等待可读会话"));
    // Pair a second browser through the Nomad JSON endpoint while the Kindle stays signed in.
    let json = |code: &str| serde_json::json!({"code": code}).to_string();
    assert!(send(&reader, "/pair", "", "", &json(&code)).starts_with("HTTP/1.1 403"));
    let paired = send(&reader, "/pair", "", "", &json(&reader.new_code()));
    assert!(paired.starts_with("HTTP/1.1 200"));
    let nomad = cookie(&paired);
    assert_ne!(nomad, device);
    assert!(send(&reader, "/sessions", nomad, "", "").starts_with("HTTP/1.1 200"));
    let mut convo = Conversation::new("s", "项目", "样本");
    convo.put(
        "q",
        "user",
        "## 最早的提问\n\n- **强调** 和 `行内代码`".into(),
    );
    let table = format!(
        "|项目|说明|\n|---|---|\n|起点|{}|\n|终点|同一张表|",
        "🦀".repeat(560)
    );
    convo.put("a1", "assistant", table);
    convo.put("a2", "assistant", "第一轮补充".into());
    convo.put("q2", "user", "第二轮提问".into());
    let long = "最后一行".to_owned();
    convo.put("a / & 字", "assistant", long.clone());
    lib.lock().unwrap().add(convo);
    let list = get("/kindle");
    assert!(list.contains("<small>项目 · 样本</small>"));
    let path = attr(&list, r#"class="row"><a href=""#);
    let latest = get(path);
    assert!(latest.contains("最后一行") && !latest.contains('🦀'));
    let follow = |html: &str, rel: &str| {
        get(&attr(html, &format!("rel=\"{rel}\" href=\"")).replace("&amp;", "&"))
    };
    let first = follow(&latest, "prev");
    assert!(first.contains("<h2>最早的提问</h2>") && first.contains("第一轮补充"));
    assert!(!first.contains("第二轮提问") && latest.contains("第二轮提问"));
    assert!(first.contains("<strong>强调</strong>") && first.contains("<code>行内代码</code>"));
    assert_eq!(first.matches("<table>").count(), 1);
    assert!(first.contains("<td>起点</td>") && first.contains("<td>终点</td>"));
    assert_eq!(first.matches('🦀').count(), 560);
    assert_eq!(attr(&follow(&first, "next"), "data-tail=\""), "true");
    for page in [&first, &latest] {
        let nav = page
            .split_once("<nav")
            .unwrap()
            .1
            .split_once("</nav>")
            .unwrap()
            .0;
        for label in ["上一屏", "下一屏", "查看最新", "返回列表"] {
            assert_eq!(nav.matches(label).count(), 1);
        }
        assert_eq!(nav.matches("aria-disabled=\"true\"").count(), 1);
    }
    assert!(get(&format!("{path}?id=a1&part=1")).contains("第一轮补充"));
    assert!(get(&format!("{path}?id=q2&part=0&step=prev")).contains("第一轮补充"));
    assert!(get(&format!("{path}?id=a2&part=0&step=next")).contains("第二轮提问"));
    let rev = attr(&latest, "data-revision=\"");
    assert!(send(&reader, path, device, rev, "").starts_with("HTTP/1.1 204"));
    lib.lock().unwrap().sessions.get_mut("s").unwrap().put(
        "a / & 字",
        "assistant",
        long + "**更新**<追加>",
    );
    let status = send(
        &reader,
        path,
        device,
        &format!("{rev}\r\nX-Kindle-Status-Only: 1"),
        "",
    );
    let status: serde_json::Value =
        serde_json::from_str(status.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(status["turn"], "q2");
    assert!(status.get("html").is_none());
    let update = send(&reader, path, device, rev, "");
    let json: serde_json::Value =
        serde_json::from_str(update.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert!(json["html"]
        .as_str()
        .unwrap()
        .contains("最后一行<strong>更新</strong>&lt;追加&gt;"));
    reader.revoke().unwrap();
    assert!(send(&reader, "/sessions", nomad, "", "").starts_with("HTTP/1.1 401"));
    assert!(get(path).contains("六位配对码") && !get(path).contains("最后一行"));
    assert!(send(&reader, path, device, rev, "").starts_with("HTTP/1.1 401"));
}

#[test]
fn kindle_native_reply_form_sends_once_and_obeys_mac_permission() {
    use red_green::reader::Source;
    use serde_json::{json, Value};
    struct Inbox(Arc<Mutex<Vec<String>>>);
    impl Source for Inbox {
        fn sessions(&mut self) -> Value {
            json!([])
        }
        fn messages(&mut self, _: &str, _: &str) -> Result<Value, u16> {
            Ok(json!({}))
        }
        fn kindle_messages(&mut self, _: &str, _: &str) -> Result<Value, u16> {
            Ok(json!({"revision":"1","title":"测试对话","tail":true,"messages":[]}))
        }
        fn send_reply(
            &mut self,
            _: &str,
            text: &str,
            admit: impl FnOnce() -> Result<(), u16>,
        ) -> Result<(), u16> {
            if text.trim() == "忙" {
                return Err(409);
            }
            admit()?;
            self.0.lock().unwrap().push(text.into());
            if text == "回执未知" {
                return Err(502);
            }
            Ok(())
        }
    }
    let sent = Arc::new(Mutex::new(vec![]));
    let reader = Reader::start("127.0.0.1:0".parse().unwrap(), Inbox(sent.clone())).unwrap();
    let code = reader.new_code();
    let pairing = send(&reader, "/kindle", "", "", "");
    let nonce = attr(&pairing, r#"name="form" value=""#);
    let paired = send(
        &reader,
        "/kindle/pair",
        cookie(&pairing),
        "",
        &format!("form={nonce}&code={code}"),
    );
    let device = cookie(&paired);
    let path = "/kindle/reply/codex-demo";
    let get = |p| send(&reader, p, device, "", "");
    let post = |body: &str| send(&reader, path, device, "", body);
    assert!(get(path).starts_with("HTTP/1.1 403"));
    reader.toggle_replies();
    let thread = get("/kindle/read/codex-demo");
    assert!(thread.contains(r#"href="/kindle/reply/codex-demo">回复</a>"#));
    assert!(thread.find("id=\"kindle-reply\"").unwrap() < thread.find("rel=\"prev\"").unwrap());
    assert!(!get("/kindle/read/claude-demo").contains("/kindle/reply/"));
    let form = get(path);
    assert!(form.contains("<textarea") && form.contains("测试对话"));
    let ticket = attr(&form, r#"name="ticket" value=""#);
    assert!(post(&format!("ticket={ticket}&text=+")).starts_with("HTTP/1.1 422"));
    assert!(post(&format!("ticket={ticket}&text=忙")).starts_with("HTTP/1.1 409"));
    let leading = post(&format!("ticket={ticket}&text=%0D%0A忙"));
    assert!(leading.contains("required>\n\n忙</textarea>"));
    let body = format!("ticket={ticket}&text=你好%0D%0A第二行%26%3C");
    assert!(
        send(&reader, "/kindle/reply/codex-other", device, "", &body).starts_with("HTTP/1.1 412")
    );
    assert!(send_from(
        &reader,
        path,
        device,
        "",
        &body,
        "Origin: http://other.invalid\r\n"
    )
    .starts_with("HTTP/1.1 403"));
    let ok = send_from(
        &reader,
        path,
        device,
        "",
        &body,
        &format!("Referer: http://{}{path}\r\n", reader.address()),
    );
    assert!(
        ok.starts_with("HTTP/1.1 303") && ok.contains("Location: /kindle/read/codex-demo?latest=1")
    );
    assert!(post(&body).starts_with("HTTP/1.1 412"));
    let ticket_for = || attr(&get(path), r#"name="ticket" value=""#).to_owned();
    let boundary = format!("{}\r\n末", "字".repeat(8190));
    let encoded = form_urlencoded::Serializer::new(String::new())
        .append_pair("ticket", &ticket_for())
        .append_pair("text", &boundary)
        .finish();
    assert!(post(&encoded).starts_with("HTTP/1.1 303"));
    let uncertain = format!("ticket={}&text=回执未知", ticket_for());
    let unknown = post(&uncertain);
    assert!(
        unknown.starts_with("HTTP/1.1 502")
            && unknown.contains("发送结果待核对")
            && !unknown.contains("<textarea")
    );
    assert!(post(&uncertain).starts_with("HTTP/1.1 412"));
    let fresh = get(path);
    let stale = format!(
        "ticket={}&text=撤销后",
        attr(&fresh, r#"name="ticket" value=""#)
    );
    reader.toggle_replies();
    let denied = post(&stale);
    assert!(denied.starts_with("HTTP/1.1 403") && denied.contains("撤销后"));
    assert_eq!(
        sent.lock().unwrap().as_slice(),
        [
            "你好\n第二行&<".to_owned(),
            boundary.replace("\r\n", "\n"),
            "回执未知".to_owned()
        ]
    );
}
