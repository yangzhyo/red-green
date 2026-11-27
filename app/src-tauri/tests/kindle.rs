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
    assert!(list.contains("项目") && list.contains("样本"));
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
    assert!(get(path).contains("六位配对码") && !get(path).contains("最后一行"));
    assert!(send(&reader, path, device, rev, "").starts_with("HTTP/1.1 401"));
}
