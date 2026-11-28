use red_green::reader::{Library, Reader};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
};

fn request(server: &Reader, method: &str, path: &str, body: &str, cookie: &str) -> String {
    let mut stream = TcpStream::connect(server.address()).unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\nOrigin: http://{}\r\nCookie: {cookie}\r\n\r\n{body}", server.address(), body.len(), server.address()).unwrap();
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
    for path in [
        "/sessions",
        "/sessions/demo/messages",
        "/history",
        "/history/status?generation=demo",
        "/history/codex-one/messages",
    ] {
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
            "key":"claude-one", "project":"同名项目", "source":"Claude Code", "state":"awaiting", "title":"第一行"
        }])
    );
    let page = reader.messages("claude-one", "").unwrap();
    assert_eq!(page["state"], "awaiting");
    assert_eq!(page["title"], "第一行");
    assert_eq!(page["messages"][0]["text"], "第一行");
    assert_eq!(page["messages"][0]["html"], "<p>第一行</p>\n");
    let since = format!("since={}", page["since"].as_str().unwrap());
    let answer = format!("## 回复\n\n{}", "一段中文".repeat(150));
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    writeln!(
        file,
        "{}",
        json!({"type":"ai-title","sessionId":"one","aiTitle":"自动名称"})
    )
    .unwrap();
    assert_eq!(reader.sessions()[0]["title"], "自动名称");
    writeln!(
        file,
        "{}",
        json!({"type":"custom-title","sessionId":"one","customTitle":"手动改名"})
    )
    .unwrap();
    writeln!(
        file,
        "{}",
        json!({"type":"ai-title","sessionId":"one","aiTitle":"更新的自动名称"})
    )
    .unwrap();
    writeln!(fs::OpenOptions::new().append(true).open(&transcript).unwrap(),"{}",json!({"type":"assistant","sessionId":"one","uuid":"a","parentUuid":"q","message":{"role":"assistant","content":answer}})).unwrap();
    let new = reader.messages("claude-one", &since).unwrap();
    assert_eq!(new["state"], "awaiting");
    assert_eq!(new["title"], "手动改名");
    assert_eq!(reader.sessions()[0]["title"], "手动改名");
    assert_eq!(new["messages"].as_array().unwrap().len(), 1);
    assert_eq!(new["messages"][0]["text"], answer);
    assert!(new["messages"][0]["html"]
        .as_str()
        .unwrap()
        .contains("<h2>回复</h2>"));
    fs::write(&transcript, "").unwrap();
    let cleared = reader.messages("claude-one", &since).unwrap();
    assert_eq!(cleared["reset"], true);
    assert_eq!(cleared["title"], "未命名会话");
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
        let mut preflights = 0;
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
                "thread/read" => {
                    preflights += 1;
                    let status = if preflights == 1 { "active" } else { "idle" };
                    json!({"thread":{"id":"one","status":{"type":status}}})
                }
                "turn/start" => {
                    assert_eq!(r["params"]["threadId"], "one");
                    let input = &r["params"]["input"][0];
                    assert_eq!(input["type"], "text");
                    match input["text"].as_str().unwrap() {
                        "明确拒绝" => {
                            ws.send(tungstenite::Message::text(
                                json!({"id":r["id"],"error":{"code":-32602,"message":"invalid"}})
                                    .to_string(),
                            ))
                            .unwrap();
                            continue;
                        }
                        "回执丢失" => break,
                        "追问" => json!({"turn":{"id":"t4"}}),
                        other => panic!("unexpected input {other}"),
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
    assert_eq!(
        reader.send_reply("codex-gone", "不可用", || panic!("未派发")),
        Err(503)
    );
    assert_eq!(
        reader.send_reply("codex-one", "运行中", || panic!("未派发")),
        Err(409)
    );
    assert_eq!(
        reader.send_reply("codex-one", "提交前撤销", || Err(403)),
        Err(403)
    );
    assert_eq!(
        reader.send_reply("codex-one", "明确拒绝", || Ok(())),
        Err(424)
    );
    assert_eq!(reader.send_reply("codex-one", "追问", || Ok(())), Ok(()));
    assert_eq!(
        reader.send_reply("codex-one", "回执丢失", || Ok(())),
        Err(502)
    );
    drop(reader);
    responder.join().unwrap();
    std::fs::remove_file(socket).unwrap();
}

#[test]
fn reply_permission_and_single_use_tickets_are_bound_to_device_and_session() {
    use red_green::reader::Source;
    use serde_json::{json, Value};
    struct Replies(Arc<Mutex<Vec<String>>>);
    impl Source for Replies {
        fn sessions(&mut self) -> Value {
            json!([])
        }
        fn messages(&mut self, _: &str, _: &str) -> Result<Value, u16> {
            Ok(json!({}))
        }
        fn history(&mut self, cursor: Option<&str>) -> Result<Value, u16> {
            Ok(json!({"entries":[],"nextCursor":cursor}))
        }
        fn history_messages(&mut self, _: &str, _: &str) -> Result<Value, u16> {
            Ok(json!({"live":false}))
        }
        fn resume(
            &mut self,
            key: &str,
            admit: impl FnOnce() -> Result<(), u16>,
        ) -> Result<Value, u16> {
            admit()?;
            self.0.lock().unwrap().push(format!("恢复 {key}"));
            Ok(json!({"ready":false}))
        }
        fn send_reply(
            &mut self,
            _: &str,
            text: &str,
            admit: impl FnOnce() -> Result<(), u16>,
        ) -> Result<(), u16> {
            admit()?;
            self.0.lock().unwrap().push(text.into());
            Ok(())
        }
    }
    let sent = Arc::new(Mutex::new(Vec::new()));
    let reader = Reader::start("127.0.0.1:0".parse().unwrap(), Replies(sent.clone())).unwrap();
    assert_eq!(reader.toggle_replies(), None);
    let code = json!({"code":reader.new_code()}).to_string();
    let response = request(&reader, "POST", "/pair", &code, "");
    let cookie = response
        .lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let one = "/sessions/codex-one/messages";
    let two = "/sessions/codex-two/messages";
    let claude = "/sessions/claude-three/messages";
    let page = |path| -> Value {
        let response = request(&reader, "GET", path, "", cookie);
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    let post =
        |path, body: &Value, cookie| request(&reader, "POST", path, &body.to_string(), cookie);
    assert!(request(
        &reader,
        "GET",
        "/history/status?generation=demo",
        "",
        cookie
    )
    .starts_with("HTTP/1.1 200"));
    assert!(
        request(&reader, "GET", "/history/status?generation=old", "", cookie)
            .starts_with("HTTP/1.1 409")
    );
    let historical = "/history/codex-one/messages";
    let resume = "/history/codex-one/resume";
    assert_eq!(
        page("/history?cursor=opaque%2B%26%25")["nextCursor"],
        "opaque+&%"
    );
    assert!(page(historical)["resumeTicket"].is_null());
    assert!(page(one)["replyTicket"].is_null());
    assert_eq!(reader.toggle_replies(), Some(true));
    assert!(page(claude)["replyTicket"].is_null());
    let body = json!({"text":"你好", "ticket":page(one)["replyTicket"]});
    assert!(post(one, &body, "").starts_with("HTTP/1.1 401"));
    assert!(post(claude, &body, cookie).starts_with("HTTP/1.1 405"));
    assert!(post(two, &body, cookie).starts_with("HTTP/1.1 412"));
    assert!(post(one, &body, cookie).starts_with("HTTP/1.1 200"));
    assert!(post(one, &body, cookie).starts_with("HTTP/1.1 412"));
    let historical_page = page(historical);
    assert!(historical_page["replyTicket"].is_null());
    let activation = json!({"ticket":historical_page["resumeTicket"]});
    let wrong_action = json!({"ticket":activation["ticket"],"text":"不能跨用途"});
    assert!(post(one, &wrong_action, cookie).starts_with("HTTP/1.1 412"));
    let wrong_ticket = json!({"ticket":page(one)["replyTicket"]});
    assert!(post(resume, &wrong_ticket, cookie).starts_with("HTTP/1.1 412"));
    assert!(post("/history/codex-two/resume", &activation, cookie).starts_with("HTTP/1.1 412"));
    assert!(post(resume, &wrong_action, cookie).starts_with("HTTP/1.1 400"));
    assert!(post(resume, &activation, "").starts_with("HTTP/1.1 401"));
    assert!(post(resume, &activation, cookie).starts_with("HTTP/1.1 200"));
    assert!(post(resume, &activation, cookie).starts_with("HTTP/1.1 412"));
    let disabled_activation = json!({"ticket":page(historical)["resumeTicket"]});
    let next = json!({"text":"再问", "ticket":page(one)["replyTicket"]});
    assert_eq!(reader.toggle_replies(), Some(false));
    assert!(post(one, &next, cookie).starts_with("HTTP/1.1 403"));
    assert!(post(resume, &disabled_activation, cookie).starts_with("HTTP/1.1 403"));
    reader.toggle_replies();
    assert!(post(one, &next, cookie).starts_with("HTTP/1.1 412"));
    reader.new_code();
    assert!(page(one)["replyTicket"].is_null());
    reader.toggle_replies();
    reader.revoke().unwrap();
    assert!(post(one, &next, cookie).starts_with("HTTP/1.1 401"));
    assert_eq!(*sent.lock().unwrap(), ["你好", "恢复 codex-one"]);
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

#[test]
fn antigravity_and_claude_coexist_on_nomad_and_kindle_endpoints() {
    use red_green::reader::live::LiveSource;
    use serde_json::json;

    static SNAP: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());
    fn snap() -> Vec<serde_json::Value> {
        SNAP.lock().unwrap().clone()
    }
    let response_body = |raw: &str| raw.split_once("\r\n\r\n").unwrap().1.to_owned();

    let dir = std::env::temp_dir().join(format!("rg-agy-http-{}", rand::random::<u64>()));
    let reader_dir = dir.join(".reader");
    std::fs::create_dir_all(&reader_dir).unwrap();

    let claude_path = dir.join("claude.jsonl");
    std::fs::write(
        &claude_path,
        format!(
            "{}\n{}\n",
            json!({"type":"user","sessionId":"c1","uuid":"u1","message":{"role":"user","content":"Claude 提问"}}),
            json!({"type":"assistant","sessionId":"c1","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","content":"Claude 回复"}})
        ),
    )
    .unwrap();
    std::fs::write(reader_dir.join("c1"), format!("{}\n", claude_path.display())).unwrap();

    let agy_path = dir.join("transcript_full.jsonl");
    std::fs::write(
        &agy_path,
        format!(
            "{}\n{}\n",
            json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>\nAGY 提问\n</USER_REQUEST>"}),
            json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","content":"AGY 回复"})
        ),
    )
    .unwrap();
    std::fs::write(
        reader_dir.join("antigravity-cid-42"),
        format!("{}\n", agy_path.display()),
    )
    .unwrap();

    *SNAP.lock().unwrap() = vec![
        json!({"session_id":"c1","project":"alpha","state":"idle"}),
        json!({"session_id":"pid-99","source":"antigravity","conversation_id":"cid-42","title":"AGY 调研","project":"red-green","state":"awaiting"}),
    ];

    let source = LiveSource::new(snap, dir.clone(), dir.join("unused.sock"));
    let reader = Reader::start("127.0.0.1:0".parse().unwrap(), source).unwrap();
    let code = reader.new_code();
    let paired = request(&reader, "POST", "/pair", &json!({"code": code}).to_string(), "");
    let cookie = paired
        .lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap();

    let sessions: serde_json::Value =
        serde_json::from_str(&response_body(&request(&reader, "GET", "/sessions", "", cookie)))
            .unwrap();
    let list = sessions.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert!(list
        .iter()
        .any(|e| e["key"] == "claude-c1" && e["source"] == "Claude Code"));
    assert!(list.iter().any(|e| {
        e["key"] == "antigravity-cid-42"
            && e["source"] == "Antigravity CLI"
            && e["state"] == "awaiting"
            && e["title"] == "AGY 调研"
    }));

    let nomad_doc = request(&reader, "GET", "/read/current/antigravity-cid-42", "", cookie);
    assert!(nomad_doc.contains("<title>Antigravity CLI · 待确认 · red-green · AGY 调研</title>"));

    let nomad_msgs: serde_json::Value = serde_json::from_str(&response_body(&request(
        &reader,
        "GET",
        "/sessions/antigravity-cid-42/messages",
        "",
        cookie,
    )))
    .unwrap();
    assert_eq!(nomad_msgs["messages"][0]["text"], "AGY 提问");
    assert_eq!(nomad_msgs["messages"][1]["text"], "AGY 回复");

    let kindle_list = request(&reader, "GET", "/kindle", "", cookie);
    assert!(
        kindle_list.contains("AGY 调研")
            && kindle_list.contains("Antigravity CLI")
            && kindle_list.contains("Claude Code")
    );

    let kindle_page = request(&reader, "GET", "/kindle/read/antigravity-cid-42", "", cookie);
    assert!(
        kindle_page.contains("AGY 提问")
            && kindle_page.contains("AGY 回复")
            && kindle_page.contains("待确认")
    );
    assert!(!kindle_page.contains("kindle-reply"));

    // 退出后返回 410，且另一来源仍不受影响
    *SNAP.lock().unwrap() = vec![json!({"session_id":"c1","project":"alpha","state":"idle"})];
    assert!(request(
        &reader,
        "GET",
        "/sessions/antigravity-cid-42/messages",
        "",
        cookie
    )
    .starts_with("HTTP/1.1 410"));
    assert!(request(&reader, "GET", "/sessions/claude-c1/messages", "", cookie)
        .starts_with("HTTP/1.1 200"));

    drop(reader);
    let _ = std::fs::remove_dir_all(dir);
}
