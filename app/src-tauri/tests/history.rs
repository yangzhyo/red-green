use red_green::reader::{
    live::{LiveSource, ResumeTerminal},
    Source,
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    fs,
    io::Write,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

const ID: &str = "019a1234-1234-7123-8123-123456789abc";
const OTHER: &str = "019a1234-1234-7123-8123-123456789abd";
const KEY: &str = "codex-019a1234-1234-7123-8123-123456789abc";

thread_local! {
    static SNAPSHOT: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

struct Backend {
    root: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

// 用真实 Unix WebSocket 验证 Reader 与 App Server 的请求边界。
impl Backend {
    fn new(handler: impl Fn(&str, &Value, &Path) -> (Value, bool) + Send + Sync + 'static) -> Self {
        Self::with_agent(
            Some("codex-tui/0.172.0 (Mac OS; arm64) (red_green; 0.1.0)"),
            handler,
        )
    }

    fn with_agent(
        agent: Option<&'static str>,
        handler: impl Fn(&str, &Value, &Path) -> (Value, bool) + Send + Sync + 'static,
    ) -> Self {
        let root = PathBuf::from("/tmp").join(format!("rg-history-{:x}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        let socket = root.join("backend.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let cwd = root.clone();
        let worker = thread::spawn(move || {
            let handler = Arc::new(handler);
            let mut clients = Vec::new();
            for stream in listener.incoming() {
                if done.load(Ordering::Acquire) {
                    break;
                }
                let stream = stream.unwrap();
                let handler = handler.clone();
                let cwd = cwd.clone();
                clients.push(thread::spawn(move || {
                    let mut ws = tungstenite::accept(stream).unwrap();
                    while let Ok(message) = ws.read() {
                        let r: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                        let (data, switched) = match r["method"].as_str().unwrap() {
                            "initialize" => (json!({"userAgent":agent}), false),
                            "initialized" => continue,
                            method => handler(method, &r["params"], &cwd),
                        };
                        if switched {
                            ws.send(tungstenite::Message::text(
                                json!({"method":"account/updated"}).to_string(),
                            ))
                            .unwrap();
                        }
                        // 真实共享服务会在正文请求期间通知同一账号的工作区路由已就绪。
                        if r["method"] == "thread/turns/list" {
                            let notification = json!({"method":"account/updated",
                                "params":{"reason":"workspaceRoutingReady"}});
                            ws.send(tungstenite::Message::text(notification.to_string()))
                                .unwrap();
                        }
                        if data.is_null() {
                            break; // 模拟共享服务重启，关闭已有连接。
                        }
                        ws.send(tungstenite::Message::text(
                            json!({"id":r["id"],"result":data}).to_string(),
                        ))
                        .unwrap();
                    }
                }));
            }
            for client in clients {
                client.join().unwrap();
            }
        });
        Self {
            root,
            socket,
            stop,
            worker: Some(worker),
        }
    }

    fn reader(&self) -> LiveSource {
        SNAPSHOT.with_borrow_mut(Vec::clear);
        LiveSource::new(
            || SNAPSHOT.with_borrow(Clone::clone),
            self.root.clone(),
            self.socket.clone(),
        )
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = UnixStream::connect(&self.socket);
        let result = self.worker.take().unwrap().join();
        fs::remove_dir_all(&self.root).unwrap();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn row(id: &str, cwd: &Path) -> Value {
    json!({"id":id,"cwd":cwd,"name":"会话标题","preview":"初始问题","updatedAt":1234,
        "source":"cli","ephemeral":false,"parentThreadId":null,"attachedToThreadId":null,
        "status":{"type":"notLoaded"}})
}

fn account_scope(params: &Value) {
    assert_eq!(params["currentAccountOnly"], true);
    assert_eq!(params["archived"], false);
    assert_eq!(params["sourceKinds"], json!(["cli", "vscode"]));
}

#[test]
fn current_directory_and_message_updates_follow_the_observed_codex_name() {
    let server = Backend::new(|method, _, _| {
        assert_eq!(method, "thread/turns/list");
        (json!({"data":[]}), false)
    });
    let mut reader = server.reader();
    let mut thread = row(ID, Path::new("/example/project"));
    thread["status"] = json!({"type":"idle"});
    let poll = |thread: &Value| {
        let session = red_green::codex::model::session(
            thread,
            Some(&json!({"id":"t"})),
            "/dev/ttys001",
            "project",
        );
        SNAPSHOT.with_borrow_mut(|rows| *rows = vec![session]);
    };
    poll(&thread);
    assert_eq!(reader.sessions()[0]["title"], "会话标题");
    let first = reader.messages(KEY, "").unwrap();
    assert_eq!(first["title"], "会话标题");
    thread["name"] = json!("新名称");
    poll(&thread);
    let since = format!("since={}", first["since"].as_str().unwrap());
    assert_eq!(reader.messages(KEY, &since).unwrap()["title"], "新名称");
}

#[test]
fn history_document_metadata_checks_the_account_and_uses_the_bound_terminal_state() {
    let server = Backend::new(|method, params, cwd| {
        assert_eq!(method, "thread/list");
        account_scope(params);
        let id = &params["includedThreadIds"][0];
        let rows = if id == ID { vec![row(ID, cwd)] } else { vec![] };
        (json!({"data":rows}), false)
    });
    let mut reader = server.reader();
    let entry = reader.history_entry(KEY).unwrap();
    assert_eq!(entry["title"], "会话标题");
    assert_eq!(entry["source"], "Codex CLI");
    assert!(entry["state"].is_null());
    SNAPSHOT.with_borrow_mut(|sessions| sessions.push(
        json!({"source":"codex","session_id":KEY,"tty":"/dev/ttys001","state":"awaiting","reader_has_turn":true})));
    assert_eq!(reader.history_entry(KEY).unwrap()["state"], "awaiting");
    assert_eq!(reader.history_entry(&format!("codex-{OTHER}")), Err(404));
}

#[test]
fn unknown_or_older_servers_cannot_read_or_activate_history() {
    for agent in [
        None,
        Some("red_green/0.162.1 (Mac OS; arm64)"),
        Some("red_green/0.171.0-alpha.1 (Mac OS; arm64)"),
        Some("red_green/0.+171.0 (Mac OS; arm64)"),
    ] {
        let server = Backend::with_agent(agent, |method, _, _| {
            panic!("未确认账号筛选能力时不能调用 {method}")
        });
        let mut reader = server.reader();
        assert_eq!(reader.history(None), Err(501));
        assert_eq!(reader.history_messages(KEY, ""), Err(501));
        assert_eq!(reader.resume(KEY, || panic!("不能启动终端")), Err(501));
    }
}

#[test]
fn lists_only_persisted_main_cli_threads_and_preserves_the_server_cursor() {
    let server = Backend::new(|method, params, _| {
        assert_eq!(method, "thread/list");
        account_scope(params);
        assert_eq!(params["sortKey"], "updated_at");
        assert_eq!(params["sortDirection"], "desc");
        assert!(params["limit"].as_u64().is_some_and(|n| n > 0));
        let main = row(ID, Path::new("/removed/project"));
        if params["cursor"].is_null() {
            let mut gui = main;
            gui["source"] = "vscode".into();
            gui["originator"] = "readmate-history".into();
            return (json!({"data":[gui],"nextCursor":"cli-page"}), false);
        }
        if params["cursor"] == "older+page" {
            let mut second = main;
            second["id"] = OTHER.into();
            second["name"] = Value::Null;
            second["source"] = "vscode".into();
            second["originator"] = "codex-tui".into();
            return (json!({"data":[second],"nextCursor":null}), false);
        }
        assert_eq!(params["cursor"], "cli-page");
        let mut rows = vec![main.clone()];
        for (field, value) in [
            ("source", json!("exec")),
            ("source", json!({"subagent":"review"})),
            ("ephemeral", json!(true)),
            ("parentThreadId", json!(ID)),
            ("attachedToThreadId", json!(ID)),
            ("id", json!("bad-id")),
        ] {
            let mut skip = main.clone();
            skip[field] = value;
            rows.push(skip);
        }
        (json!({"data":rows,"nextCursor":"older+page"}), false)
    });
    let mut reader = server.reader();
    let first = reader.history(None).unwrap();
    assert_eq!(
        first["entries"],
        json!([{"key":KEY,"project":"project",
        "title":"会话标题","updatedAt":1234,"source":"Codex CLI"}])
    );
    let next = reader
        .history(Some(first["nextCursor"].as_str().unwrap()))
        .unwrap();
    assert_eq!(next["entries"][0]["title"], "初始问题");
    assert_eq!(next["entries"][0]["key"], format!("codex-{OTHER}"));
    assert!(next["nextCursor"].is_null());
    assert_eq!(reader.sessions(), json!([]));
}

#[test]
fn directory_paging_and_heartbeats_cannot_mix_accounts_or_service_connections() {
    let state = Arc::new(Mutex::new("initialize"));
    let mode = state.clone();
    let server = Backend::new(move |method, params, cwd| {
        let mut mode = mode.lock().unwrap();
        let switched = match *mode {
            "disconnected" => return (Value::Null, false),
            "initialize" => {
                *mode = "A";
                true
            }
            "switch" => {
                *mode = "B";
                true
            }
            _ => false,
        };
        match method {
            "thread/list" => {
                account_scope(params);
                let id = if *mode == "A" { ID } else { OTHER };
                (
                    json!({"data":[row(id,cwd)],"nextCursor":"same-backend-cursor"}),
                    switched,
                )
            }
            "thread/loaded/list" => {
                assert_eq!(params["limit"], 1);
                (json!({"data":[]}), switched)
            }
            other => panic!("目录不应读取正文：{other}"),
        }
    });
    let mut reader = server.reader();
    assert_eq!(reader.history(Some("obsolete")), Err(409));
    let first = reader.history(None).unwrap();
    let cursor_a = first["nextCursor"].as_str().unwrap();
    let generation_a = first["generation"].as_str().unwrap();
    assert_eq!(
        reader.history(None).unwrap()["generation"],
        generation_a,
        "第二个页签复用目录"
    );
    assert_eq!(reader.history_status(generation_a), Ok(json!({})));
    *state.lock().unwrap() = "switch";
    assert_eq!(reader.history(Some(cursor_a)), Err(409));
    assert_eq!(reader.history_status(generation_a), Err(409));
    let second = reader.history(None).unwrap();
    let generation_b = second["generation"].as_str().unwrap();
    assert_ne!(generation_b, generation_a);
    assert_eq!(second["entries"][0]["key"], format!("codex-{OTHER}"));
    assert_eq!(reader.history(Some(cursor_a)), Err(409));
    assert_eq!(reader.history_status(generation_a), Err(409));
    assert_eq!(reader.history_status(generation_b), Ok(json!({})));
    *state.lock().unwrap() = "switch";
    assert_eq!(reader.history_status(generation_b), Err(409));
    let third = reader.history(None).unwrap();
    *state.lock().unwrap() = "disconnected";
    assert_eq!(
        reader.history_status(third["generation"].as_str().unwrap()),
        Err(409)
    );
    assert_eq!(
        reader.history(Some(third["nextCursor"].as_str().unwrap())),
        Err(409)
    );
}

#[test]
fn resumes_the_original_terminal_once_and_only_after_the_final_grant_check() {
    let directory_exists = Arc::new(AtomicBool::new(false));
    let exists = directory_exists.clone();
    let server = Backend::new(move |method, params, cwd| {
        if method == "thread/loaded/list" {
            if cwd.join("switch-during-lookup").exists() {
                fs::rename(
                    cwd.join("switch-during-lookup"),
                    cwd.join("account-switched"),
                )
                .unwrap();
            }
            if !cwd.join("native-bound").exists() {
                return (json!({"data":[]}), false);
            }
            return if params["cursor"].is_null() {
                (json!({"data":[OTHER],"nextCursor":"more-ids"}), false)
            } else {
                (json!({"data":[ID]}), false)
            };
        }
        assert_eq!(method, "thread/list");
        account_scope(params);
        assert_eq!(params["includedThreadIds"], json!([ID]));
        let mut thread = row(ID, cwd);
        thread["source"] = "vscode".into();
        thread["originator"] = "codex-tui".into();
        if !exists.load(Ordering::Acquire) {
            thread["cwd"] = json!(cwd.join("missing"));
        }
        (
            json!({"data":[thread]}),
            cwd.join("account-switched").exists(),
        )
    });
    fn launch(cwd: &Path, id: &str, socket: &Path) -> Result<ResumeTerminal, u16> {
        assert_eq!(cwd.join("backend.sock"), socket);
        assert_eq!(id, ID);
        if !cwd.join("allow-launch").is_file() {
            return Err(424);
        }
        writeln!(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(cwd.join("launches"))
                .unwrap(),
            "{id}"
        )
        .unwrap();
        let tab = cwd.join("tab-open");
        fs::write(&tab, "").unwrap();
        Ok(ResumeTerminal::Ghostty(tab.to_str().unwrap().into()))
    }
    fn tab_exists(tab: &ResumeTerminal) -> Result<bool, u16> {
        let ResumeTerminal::Ghostty(tab) = tab else {
            panic!("意外的终端种类")
        };
        let tab = Path::new(tab);
        if tab.with_file_name("probe-error").exists() {
            return Err(503);
        }
        Ok(tab.exists())
    }
    let mut reader = server
        .reader()
        .with_terminal_launcher(launch, tab_exists, |id, loaded| {
            assert_eq!(id, ID);
            assert_eq!(loaded, &[OTHER.to_string(), ID.to_string()]);
            Ok(true)
        });
    assert_eq!(reader.resume(KEY, || panic!("无目录不应启动")), Err(422));
    // 观察器已绑定终端但还没有读到首个回合，等待该终端即可。
    SNAPSHOT.with_borrow_mut(|sessions| {
        sessions.push(
            json!({"source":"codex","session_id":KEY,"tty":"/dev/ttys001",
            "reader_has_turn":false}),
        )
    });
    assert_eq!(reader.resume(KEY, || Ok(())), Ok(json!({"ready":false})));
    SNAPSHOT.with_borrow_mut(Vec::clear);
    directory_exists.store(true, Ordering::Release);
    assert_eq!(reader.resume(KEY, || Ok(())), Err(424));
    fs::write(server.root.join("allow-launch"), "").unwrap();
    fs::write(server.root.join("switch-during-lookup"), "").unwrap();
    assert_eq!(
        reader.resume(KEY, || panic!("切换账号后不能派发")),
        Err(409)
    );
    fs::remove_file(server.root.join("account-switched")).unwrap();
    assert_eq!(reader.resume(KEY, || Err(403)), Err(403));
    assert!(!server.root.join("launches").exists());
    for _ in 0..2 {
        assert_eq!(
            reader.resume(KEY, || Ok(())).unwrap(),
            json!({"ready":false})
        );
    }
    fs::write(server.root.join("probe-error"), "").unwrap();
    assert_eq!(reader.resume(KEY, || Ok(())), Err(503));
    fs::remove_file(server.root.join("probe-error")).unwrap();
    assert_eq!(reader.resume(KEY, || Ok(())), Ok(json!({"ready":false})));
    // 关闭首个标签后，若另一个真实终端仍有该线程，即使此刻观察快照为空也要复用。
    fs::remove_file(server.root.join("tab-open")).unwrap();
    fs::write(server.root.join("native-bound"), "").unwrap();
    assert_eq!(reader.resume(KEY, || Ok(())), Ok(json!({"ready":false})));
    assert_eq!(
        fs::read_to_string(server.root.join("launches")).unwrap(),
        format!("{ID}\n")
    );
    fs::remove_file(server.root.join("native-bound")).unwrap();
    assert_eq!(reader.resume(KEY, || Ok(())), Ok(json!({"ready":false})));
    SNAPSHOT.with_borrow_mut(|sessions| {
        sessions.push(json!({"source":"codex","session_id":KEY,
        "project":"已绑定","tty":"/dev/ttys001","state":"idle","reader_has_turn":true}))
    });
    assert_eq!(reader.resume(KEY, || Err(403)), Err(403));
    assert_eq!(
        reader.resume(KEY, || Ok(())).unwrap(),
        json!({"ready":true})
    );
    assert_eq!(
        fs::read_to_string(server.root.join("launches")).unwrap(),
        format!("{ID}\n{ID}\n")
    );
}

#[test]
fn an_unknown_launch_rechecks_the_thread_and_can_be_reset_on_the_mac() {
    let bound = Arc::new(AtomicBool::new(false));
    let observed = bound.clone();
    let server = Backend::new(move |method, _, cwd| match method {
        "thread/list" => (json!({"data":[row(ID,cwd)]}), false),
        "thread/loaded/list" => {
            let ids = if observed.load(Ordering::Acquire) {
                vec![ID]
            } else {
                vec![]
            };
            (json!({"data":ids}), false)
        }
        _ => panic!("unexpected method {method}"),
    });
    let open = || {
        server.reader().with_terminal_launcher(
            |_, _, _| Ok(ResumeTerminal::Unknown),
            |_| Err(428),
            |_, _| Ok(true),
        )
    };
    let mut first = open();
    assert_eq!(first.resume(KEY, || Ok(())), Ok(json!({"ready":false})));
    assert_eq!(first.resume(KEY, || panic!("不能重复启动")), Err(428));
    bound.store(true, Ordering::Release);
    assert_eq!(first.resume(KEY, || Ok(())), Ok(json!({"ready":false})));
    drop(first);
    bound.store(false, Ordering::Release);
    assert_eq!(open().resume(KEY, || Ok(())), Ok(json!({"ready":false})));
}

#[test]
fn history_keeps_routing_ready_pages_but_discards_account_changes() {
    let access = Arc::new(Mutex::new("visible"));
    let mode = access.clone();
    let server = Backend::new(move |method, params, cwd| {
        let mode = *mode.lock().unwrap();
        match method {
            "thread/list" => {
                account_scope(params);
                assert_eq!(params["includedThreadIds"], json!([ID]));
                let id = if mode == "hidden" { OTHER } else { ID };
                (json!({"data":[row(id,cwd)]}), false)
            }
            "thread/turns/list" => {
                assert_ne!(mode, "hidden", "账号范围以外不能取正文");
                assert_eq!(params["threadId"], ID);
                assert_eq!(params["itemsView"], "full");
                let (id, text, next) = if params["cursor"] == "before" {
                    ("1", "上文", None)
                } else {
                    ("2", "**答复**", Some("before"))
                };
                (
                    json!({"data":[{"id":id,"items":[{"type":"agentMessage","id":"a","text":text}]}],
                    "nextCursor":next}),
                    mode == "change",
                )
            }
            method => panic!("历史阅读不应调用 {method}"),
        }
    });
    let mut reader = server.reader();
    let first = reader.history_messages(KEY, "").unwrap();
    assert_eq!(first["live"], false);
    assert_eq!(
        first["messages"][0]["html"],
        "<p><strong>答复</strong></p>\n"
    );
    let older = format!("before={}", first["before"].as_str().unwrap());
    SNAPSHOT.with_borrow_mut(|rows| {
        rows.push(json!({"source":"codex","session_id":KEY,"tty":"/dev/ttys001","state":"running"}))
    });
    let since = format!("since={}", first["since"].as_str().unwrap());
    let update = reader.history_messages(KEY, &since).unwrap();
    assert_eq!(update["messages"], json!([]));
    assert_eq!(update["live"], true);
    assert_eq!(update["state"], "running");
    SNAPSHOT.with_borrow_mut(|rows| rows[0]["state"] = json!("awaiting"));
    assert_eq!(
        reader.history_messages(KEY, &since).unwrap()["state"],
        "awaiting"
    );
    SNAPSHOT.with_borrow_mut(|rows| rows[0]["reader_has_turn"] = json!(false));
    let pending = reader.history_messages(KEY, &since).unwrap();
    assert_eq!(pending["live"], false);
    assert!(pending["state"].is_null());
    assert_eq!(
        reader.history_messages(KEY, &older).unwrap()["messages"][0]["text"],
        "上文"
    );
    *access.lock().unwrap() = "change";
    assert_eq!(reader.history_messages(KEY, ""), Err(409));
    *access.lock().unwrap() = "hidden";
    assert_eq!(reader.history_messages(KEY, ""), Err(404));
    assert_eq!(reader.resume(KEY, || panic!("其他账号的会话")), Err(404));
    assert_eq!(reader.history_messages("codex-invalid", ""), Err(404));
}

#[test]
fn kindle_finds_text_before_the_latest_tool_only_turns() {
    let server = Backend::new(|method, params, _| {
        assert_eq!(method, "thread/turns/list");
        let data = if params["cursor"].is_null() {
            json!({"data":[{"id":"tools","items":[]}],"nextCursor":"older"})
        } else {
            assert_eq!(params["cursor"], "older");
            json!({"data":[{"id":"answer","items":[{"id":"a","type":"agentMessage","text":"先前答复"}]}]})
        };
        (data, false)
    });
    let mut reader = server.reader();
    SNAPSHOT.with_borrow_mut(|sessions| {
        sessions.push(json!({"source":"codex","session_id":KEY,"project":"项目"}))
    });
    assert_eq!(
        reader.kindle_messages(KEY, "").unwrap()["html"]
            .as_str()
            .unwrap()
            .trim(),
        "<p>先前答复</p>"
    );
}
