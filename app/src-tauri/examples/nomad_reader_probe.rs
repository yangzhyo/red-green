//! Run a private-data-free Nomad trial, using the same HTTP service as the reader.
use red_green::reader::{Conversation, Library, Reader};
use std::{
    io::{self, BufRead},
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [flag, address] = args.as_slice() else {
        return Err("用法：nomad_reader_probe --bind IP:端口".into());
    };
    if flag != "--bind" {
        return Err("需要 --bind IP:端口".into());
    }
    let address: SocketAddr = address.parse()?;
    if address.ip().is_unspecified() {
        return Err("请选择具体的局域网地址".into());
    }
    let mut demo = Conversation::new("nomad-demo", "阅读样本", "示例");
    demo.put(
        "q1",
        "user",
        "我可以在前面读一段，再回到最新消息吗？".into(),
    );
    let mut long_text = String::from(
        "可以。用「上一屏」回看代码，记住屏幕最上方的一行。新消息到达后，你仍能从这里继续读。\n\n```text\n",
    );
    for n in 1..=60 {
        long_text.push_str(&format!("    第 {n:02} 行    阅读位置示例\n"));
    }
    long_text.push_str("```");
    demo.put("a1", "assistant", long_text);
    demo.put("q2", "user", "页面保持打开时能收到更新吗？".into());
    demo.put(
        "a2",
        "assistant",
        "### 自动刷新\n\n样本将在接下来每隔 12 秒自动追加一段文字。\n\n1. 在末尾等待，观察**新回复**。\n2. 翻到上一屏，观察阅读位置。\n\n| 阅读位置 | 新消息到达时 |\n| --- | --- |\n| 末尾 | 跟随更新 |\n| 前文 | 留在原处 |".into(),
    );
    let mut library = Library::default();
    library.add(demo);
    let library = Arc::new(Mutex::new(library));
    let reader = Reader::start(address, library.clone())?;
    println!("阅读样本入口 http://{}/", reader.address());
    println!("一次性配对码 {}（10 分钟有效）", reader.new_code());
    println!("输入文字可追加样本；输入 /code 换码、/revoke 撤销、/quit 退出。");
    thread::spawn({
        let library = library.clone();
        move || {
            for n in 1..=150 {
                thread::sleep(Duration::from_secs(12));
                let mut lib = library.lock().unwrap();
                let thread = lib.sessions.get_mut("nomad-demo").unwrap();
                let round = (n + 1) / 2;
                let text = if n % 2 == 1 {
                    format!("第 {round} 次自动更新。")
                } else {
                    format!("第 {round} 次自动更新。\n这段新文字追加在同一条回复中。")
                };
                thread.put(&format!("auto-{round}"), "assistant", text);
            }
        }
    });
    for (n, line) in io::stdin().lock().lines().enumerate() {
        let line = line?;
        match line.trim() {
            "/quit" => break,
            "/code" => println!("一次性配对码 {}", reader.new_code()),
            "/revoke" => reader.revoke()?,
            "" => (),
            _ => library
                .lock()
                .unwrap()
                .sessions
                .get_mut("nomad-demo")
                .unwrap()
                .put(&format!("typed-{n}"), "assistant", line),
        }
    }
    Ok(())
}
