use red_green::reader::{Conversation, Library, Reader};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
};

fn send(reader: &Reader, path: &str, cookie: &str, since: &str, body: &str) -> String {
    let mut tcp = TcpStream::connect(reader.address()).unwrap();
    let method = if body.is_empty() { "GET" } else { "POST" };
    let extra = if since.is_empty() {
        String::new()
    } else {
        format!("X-Kindle-Since: {since}\r\n")
    };
    write!(tcp, "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nCookie: {cookie}\r\n{extra}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",reader.address(),body.len()).unwrap();
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
    let mut convo = Conversation::new("s", "项目", "样本");
    convo.put(
        "q",
        "user",
        "## 最早的提问\n\n- **强调** 和 `行内代码`".into(),
    );
    let long = format!("{}最后一行", "🦀".repeat(560));
    convo.put("a / & 字", "assistant", long.clone());
    lib.lock().unwrap().add(convo);
    let list = get("/kindle");
    assert!(list.contains("项目") && list.contains("样本"));
    let path = attr(&list, r#"class="row"><a href=""#);
    let latest = get(path);
    assert!(latest.contains("最后一行") && !latest.contains('🦀'));
    let follow = |html: &str, rel: &str| {
        get(&attr(html, &format!("rel=\"{rel}\" href=\"")).replace("&amp;", "&"))
    };
    let middle = follow(&latest, "prev");
    let first = follow(&middle, "prev");
    let question = follow(&first, "prev");
    assert!(question.contains("<h2>最早的提问</h2>") && !question.contains("rel=\"prev\""));
    assert!(
        question.contains("<ul>")
            && question.contains("<strong>强调</strong>")
            && question.contains("<code>行内代码</code>")
    );
    assert_eq!(attr(&follow(&question, "next"), "data-tail=\""), "false");
    let part = |html: &str| {
        html.split_once(r#"<div class="markdown"><p>"#)
            .unwrap()
            .1
            .split_once("</p>")
            .unwrap()
            .0
            .to_owned()
    };
    assert_eq!(part(&first) + &part(&middle) + &part(&latest), long);
    assert_eq!(part(&follow(&first, "next")), part(&middle));
    let rev = attr(&latest, "data-revision=\"");
    assert!(send(&reader, path, device, rev, "").starts_with("HTTP/1.1 204"));
    lib.lock().unwrap().sessions.get_mut("s").unwrap().put(
        "a / & 字",
        "assistant",
        long + "**更新**<追加>",
    );
    let update = send(&reader, path, device, rev, "");
    let json: serde_json::Value =
        serde_json::from_str(update.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert!(json["html"]
        .as_str()
        .unwrap()
        .contains("最后一行<strong>更新</strong>&lt;追加&gt;"));
    reader.revoke().unwrap();
    assert!(get(path).contains("六位配对码") && !get(path).contains("最后一行"));
    assert!(send(&reader, path, device, rev, "").starts_with("HTTP/1.1 401"));
}
