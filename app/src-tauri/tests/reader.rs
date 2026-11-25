use red_green::reader::{Library, Reader};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
};

fn request(server: &Reader, method: &str, path: &str, body: &str, cookie: &str) -> String {
    let mut stream = TcpStream::connect(server.address()).unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\nCookie: {cookie}\r\n\r\n{body}", server.address(), body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn reader_pairing_and_revocation() {
    let reader = Reader::start(
        "127.0.0.1:0".parse().unwrap(),
        Arc::new(Mutex::new(Library::default())),
    )
    .unwrap();
    for path in ["/sessions", "/sessions/demo/messages"] {
        assert!(request(&reader, "GET", path, "", "").starts_with("HTTP/1.1 401"));
    }
    let code = reader.new_code();
    let body = serde_json::json!({"code":code}).to_string();
    let response = request(&reader, "POST", "/pair", &body, "");
    assert!(response.starts_with("HTTP/1.1 200"));
    let cookie = response
        .lines()
        .find_map(|s| s.strip_prefix("Set-Cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert!(request(&reader, "POST", "/pair", &body, "").starts_with("HTTP/1.1 403"));
    assert!(request(&reader, "GET", "/sessions", "", cookie).starts_with("HTTP/1.1 200"));
    reader.revoke().unwrap();
    assert!(request(&reader, "GET", "/sessions", "", cookie).starts_with("HTTP/1.1 401"));
}

#[test]
fn current_directory_and_incremental_claude_history() {
    use red_green::reader::{live::LiveSource, Source};
    use serde_json::json;
    use std::fs;
    let root = std::env::temp_dir().join(format!("reader-{}", rand::random::<u64>()));
    fs::create_dir_all(root.join(".reader")).unwrap();
    let transcript = root.join("fixture.jsonl");
    fs::write(
        &transcript,
        format!(
            "{}\n",
            json!({"type":"user","sessionId":"one","uuid":"q",
        "message":{"role":"user","content":"第一行"}})
        ),
    )
    .unwrap();
    let mapping = root.join(".reader/one");
    fs::write(&mapping, transcript.to_str().unwrap()).unwrap();
    let mut reader = LiveSource::new(
        || {
            vec![
                json!({"project":"同名项目","session_id":"one","state":"awaiting"}),
                json!({"project":"同名项目","session_id":"two"}),
            ]
        },
        root.clone(),
        root.join("unused"),
    );
    assert_eq!(
        reader.sessions(),
        json!([{
            "key":"claude-one", "project":"同名项目", "source":"Claude Code", "state":"awaiting"
        }])
    );
    let page = reader.messages("claude-one", "").unwrap();
    assert_eq!(page["state"], "awaiting");
    assert_eq!(page["messages"][0]["text"], "第一行");
    let since = format!("since={}", page["since"].as_str().unwrap());
    let answer = "一段中文".repeat(150);
    writeln!(fs::OpenOptions::new().append(true).open(&transcript).unwrap(),"{}",json!({"type":"assistant","sessionId":"one","uuid":"a","parentUuid":"q","message":{"role":"assistant","content":answer}})).unwrap();
    let new = reader.messages("claude-one", &since).unwrap();
    assert_eq!(new["state"], "awaiting");
    assert_eq!(new["messages"].as_array().unwrap().len(), 1);
    assert_eq!(new["messages"][0]["text"], answer);
    fs::write(&transcript, "").unwrap();
    let cleared = reader.messages("claude-one", &since).unwrap();
    assert_eq!(cleared["reset"], true);
    assert!(cleared["messages"].as_array().unwrap().is_empty());
    fs::write(mapping, "ended\n").unwrap();
    assert_eq!(reader.messages("claude-one", ""), Err(410));
    assert!(reader.sessions().as_array().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_reader_fetches_older_turns_on_demand_and_resumes_a_reply() {
    use red_green::reader::{live::LiveSource, Source};
    use serde_json::{json, Value};
    use std::os::unix::net::UnixListener;
    let socket = std::env::temp_dir().join(format!("rg-{:x}.sock", rand::random::<u64>()));
    let listener = UnixListener::bind(&socket).unwrap();
    let responder = std::thread::spawn(move || {
        let mut ws = tungstenite::accept(listener.accept().unwrap().0).unwrap();
        let mut reads = 0;
        while let Ok(frame) = ws.read() {
            let r: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            let data = match r["method"].as_str().unwrap() {
                "initialize" => json!({}),
                "initialized" => continue,
                "thread/turns/list" => {
                    assert_eq!(r["params"]["itemsView"], "full");
                    assert_eq!(r["params"]["threadId"], "one");
                    reads += 1;
                    let turn = |id: &str, text: &str| json!({"id":id,"items":[{"type":"agentMessage","id":"a","text":text}]});
                    match reads {
                        1 => {
                            ws.send(tungstenite::Message::text(
                                json!({"method":"account/updated"}).to_string(),
                            ))
                            .unwrap();
                            json!({"data":[turn("discard","作废")]})
                        }
                        2 => json!({"data":[turn("t2","半句")],"nextCursor":"earlier"}),
                        3 => {
                            assert_eq!(r["params"]["cursor"], "earlier");
                            json!({"data":[turn("t1","先前回复")]})
                        }
                        4 => json!({"data":[turn("t3","后续回复"),turn("t2","完整一句")]}),
                        _ => panic!("unexpected read"),
                    }
                }
                other => panic!("unexpected RPC {other}"),
            };
            ws.send(tungstenite::Message::text(
                json!({"id":r["id"],"result":data}).to_string(),
            ))
            .unwrap();
        }
    });
    let mut reader = LiveSource::new(
        || vec![json!({"source":"codex","project":"示例","session_id":"codex-one"})],
        std::env::temp_dir(),
        socket.clone(),
    );
    let first = reader.messages("codex-one", "").unwrap();
    assert_eq!(first["messages"][0]["text"], "半句");
    let before = format!("before={}", first["before"].as_str().unwrap());
    let earlier = reader.messages("codex-one", &before).unwrap();
    assert_eq!(earlier["messages"][0]["text"], "先前回复");
    assert!(earlier["before"].is_null());
    let new = reader
        .messages(
            "codex-one",
            &format!("since={}", first["since"].as_str().unwrap()),
        )
        .unwrap();
    assert_eq!(new["messages"][0]["text"], "完整一句");
    assert_eq!(new["messages"][1]["text"], "后续回复");
    drop(reader);
    responder.join().unwrap();
    std::fs::remove_file(socket).unwrap();
}

#[test]
fn an_unmaterialized_codex_session_waits_outside_the_readable_directory() {
    use red_green::reader::{live::LiveSource, Source};
    let mut reader = LiveSource::new(
        || {
            vec![
                serde_json::json!({"source":"codex","project":"尚无回合","session_id":"codex-one","reader_has_turn":false}),
            ]
        },
        std::env::temp_dir(),
        "unused".into(),
    );
    assert_eq!(reader.sessions(), serde_json::json!([]));
}
