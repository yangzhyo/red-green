//! PTY 集成测试驱动真实输出循环；Desktop 仅记录副作用，不创建系统窗口。
use red_green::output::{
    model::{Call, OutputSnapshot},
    runtime::{self, Command, Desktop},
};
use serde_json::json;
use std::io::{self, BufRead};
struct Observer;
impl Desktop for Observer {
    fn reconcile(&mut self, s: &OutputSnapshot) -> Result<(), String> {
        println!(
            "{}",
            json!({"desktop":"show","pets":s.pets.iter().map(|p|&p.sid).collect::<Vec<_>>(),"at":runtime::now_ms()})
        );
        Ok(())
    }
    fn quiesce(&mut self) -> Result<(), String> {
        println!("{}", json!({"desktop":"stop","at":runtime::now_ms()}));
        Ok(())
    }
    fn play(&mut self, name: &str) -> Result<(), String> {
        println!(
            "{}",
            json!({"desktop":"call","name":name,"at":runtime::now_ms()})
        );
        Ok(())
    }
}
fn main() {
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    std::thread::spawn(|| {
        runtime::run(Observer, rx, |line| {
            println!("{}", json!({"log":line,"at":runtime::now_ms()}))
        })
    });
    for line in io::stdin().lock().lines() {
        let v: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let snapshot: OutputSnapshot = serde_json::from_value(v["snapshot"].clone()).unwrap();
        let calls: Vec<Call> = serde_json::from_value(v["calls"].clone()).unwrap();
        tx.send(Command::Submit {
            snapshot,
            calls,
            at: runtime::now_ms(),
        })
        .unwrap();
    }
}
