use red_green::reader::{Reader, Source};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::mpsc,
    time::Duration,
};

fn request(address: SocketAddr, path: &str, cookie: &str, accept: &str, body: &str) -> String {
    let method = if body.is_empty() { "GET" } else { "POST" };
    let mut stream = TcpStream::connect(address).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nCookie: {cookie}\r\nAccept: {accept}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    ).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn pair(reader: &Reader) -> String {
    let code = json!({"code": reader.new_code()}).to_string();
    let response = request(reader.address(), "/pair", "", "application/json", &code);
    response
        .lines()
        .find_map(|line| line.strip_prefix("Set-Cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

fn html(response: &str) -> &str {
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(headers.contains("Content-Type: text/html; charset=utf-8"));
    assert!(headers.contains("Cache-Control: no-store"));
    body
}

struct Documents;
impl Source for Documents {
    fn sessions(&mut self) -> Value {
        json!([
            {"key":"claude-one", "title":"“编辑” <草案> \"v2\" & '校样'", "project":"示例 & 伙伴",
             "source":"Claude Code", "state":"awaiting", "private":"仅供本机调试"},
            {"key":"codex-same", "title":"项目\n甲", "project":"项目 甲", "source":"Codex CLI", "state":"idle"},
            {"key":"codex-unnamed", "project":"示例", "source":"Codex CLI"}
        ])
    }

    fn history_entry(&mut self, key: &str) -> Result<Value, u16> {
        match key {
            "codex-archive" | "codex-ready" => Ok(json!({
                "key":key, "title":"历史标题", "project":"历史项目", "source":"Codex CLI",
                "state":(key == "codex-ready").then_some("running")
            })),
            "codex-unavailable" => Err(503),
            _ => Err(404),
        }
    }

    fn messages(&mut self, _: &str, _: &str) -> Result<Value, u16> {
        panic!("生成详情首屏不应读取正文")
    }
}

#[test]
fn documents_serve_native_titles_and_only_reader_metadata() {
    let reader = Reader::start("127.0.0.1:0".parse().unwrap(), Documents).unwrap();
    let cookie = pair(&reader);
    let get = |path| request(reader.address(), path, &cookie, "text/html", "");
    assert_eq!(html(&get("/")), include_str!("../../ui/reader.html"));
    let pairing = request(reader.address(), "/pair", "", "text/html", "");
    assert!(html(&pairing).contains("<title>red-green · 设备配对</title>"));
    let archive = get("/archive");
    assert!(html(&archive).contains("<title>red-green · 历史会话</title>"));
    assert!(html(&archive).contains(r#"<body data-page="history">"#));

    let path = "/read/current/claude-one";
    let response = get(path);
    let body = html(&response);
    assert!(body.contains("<title>Claude Code · 待确认 · 示例 &amp; 伙伴 · “编辑” &lt;草案&gt; &quot;v2&quot; &amp; &#39;校样&#39;</title>"));
    let attribute = body
        .split_once(r#"<body data-page="thread" data-entry=""#)
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0;
    assert!(attribute.contains("&quot;title&quot;"));
    assert!(attribute.contains("&lt;草案&gt;"));
    let decoded = attribute
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    let metadata: Value = serde_json::from_str(&decoded).unwrap();
    assert_eq!(
        metadata,
        json!({
            "key":"claude-one", "title":"“编辑” <草案> \"v2\" & '校样'", "project":"示例 & 伙伴",
            "source":"Claude Code", "state":"awaiting"
        })
    );
    assert!(!body.contains("仅供本机调试"));
    let json_response = request(reader.address(), path, &cookie, "application/json", "");
    let (headers, payload) = json_response.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(headers.contains("Content-Type: application/json"));
    assert_eq!(serde_json::from_str::<Value>(payload).unwrap(), metadata);

    for (path, title) in [
        ("/read/current/codex-same", "Codex CLI · 空闲 · 项目 甲"),
        (
            "/read/current/codex-unnamed",
            "Codex CLI · 示例 · 未命名会话",
        ),
        (
            "/read/history/codex-archive",
            "Codex CLI · 只读 · 历史项目 · 历史标题",
        ),
        (
            "/read/history/codex-ready",
            "Codex CLI · 运行中 · 历史项目 · 历史标题",
        ),
    ] {
        assert!(html(&get(path)).contains(&format!("<title>{title}</title>")));
    }
}

#[test]
fn current_document_uses_the_name_from_the_codex_snapshot() {
    use red_green::{codex::model::session, reader::live::LiveSource};
    const ID: &str = "019a1234-1234-7123-8123-123456789abc";
    let source = LiveSource::new(
        || {
            let thread = json!({"id":ID, "source":"cli", "name":"发布 <候选> & 验收",
                "preview":"预览文字", "status":{"type":"active"}});
            vec![session(
                &thread,
                Some(&json!({"id":"turn"})),
                "/dev/ttys000",
                "项目",
            )]
        },
        std::env::temp_dir(),
        "unused.sock".into(),
    );
    let reader = Reader::start("127.0.0.1:0".parse().unwrap(), source).unwrap();
    let cookie = pair(&reader);
    let response = request(
        reader.address(),
        &format!("/read/current/codex-{ID}"),
        &cookie,
        "text/html",
        "",
    );
    let body = html(&response);
    assert!(
        body.contains("<title>Codex CLI · 运行中 · 项目 · 发布 &lt;候选&gt; &amp; 验收</title>")
    );
    assert!(!body.contains("预览文字") && !body.contains("/dev/ttys000"));
}

#[test]
fn missing_unavailable_and_unpaired_details_keep_a_recoverable_shell() {
    let reader = Reader::start("127.0.0.1:0".parse().unwrap(), Documents).unwrap();
    let cookie = pair(&reader);
    for (path, cookie, code) in [
        ("/read/current/claude-one", "", 401),
        ("/read/history/codex-archive", "", 401),
        ("/read/current/claude-missing", cookie.as_str(), 404),
        ("/read/history/codex-unavailable", cookie.as_str(), 503),
    ] {
        let response = request(reader.address(), path, cookie, "text/html", "");
        let body = html(&response);
        let title = if code == 401 {
            "设备配对"
        } else {
            "会话"
        };
        assert!(body.contains(&format!("<title>red-green · {title}</title>")));
        assert!(body.contains(&format!(r#"data-reader-error="{code}""#)));
        assert!(body.contains(r#"id="pair""#));
        assert!(!body.contains("data-entry="));
        assert!(!body.contains(path.rsplit('/').next().unwrap()));
        assert!(!body.contains("草案") && !body.contains("历史标题"));
        let json_response = request(reader.address(), path, cookie, "application/json", "");
        assert!(json_response.starts_with(&format!("HTTP/1.1 {code}")));
        assert_eq!(json_response.split_once("\r\n\r\n").unwrap().1, "{}");
    }
}

#[test]
fn revoking_pairing_during_a_metadata_read_discards_the_result() {
    struct Blocking {
        reading: mpsc::Sender<()>,
        resume: mpsc::Receiver<()>,
    }
    impl Source for Blocking {
        fn sessions(&mut self) -> Value {
            self.reading.send(()).unwrap();
            self.resume.recv_timeout(Duration::from_secs(5)).unwrap();
            Documents.sessions()
        }
        fn messages(&mut self, _: &str, _: &str) -> Result<Value, u16> {
            unreachable!()
        }
    }
    let (reading, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let reader =
        Reader::start("127.0.0.1:0".parse().unwrap(), Blocking { reading, resume }).unwrap();
    let cookie = pair(&reader);
    let address = reader.address();
    let pending = std::thread::spawn(move || {
        request(
            address,
            "/read/current/claude-one",
            &cookie,
            "text/html",
            "",
        )
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    reader.revoke().unwrap();
    release.send(()).unwrap();
    let response = pending.join().unwrap();
    let body = html(&response);
    assert!(body.contains(r#"data-reader-error="401""#));
    assert!(!body.contains("data-entry=") && !body.contains("草案"));
}
