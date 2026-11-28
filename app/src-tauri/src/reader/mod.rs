pub mod antigravity;
pub mod claude;
pub mod codex;
mod history;
mod kindle;
mod markdown;
mod model;
mod pairing;
mod server;
pub use model::{Conversation, Library};
pub use server::{Reader, Source};
pub mod live;

/// 只消费以换行符结尾的完整 JSON 行，避免读取正在追加写入的半行。
pub(super) fn read_jsonl_lines(
    mut input: impl std::io::BufRead,
    mut insert: impl FnMut(&serde_json::Value),
) -> std::io::Result<usize> {
    let mut consumed = 0;
    let mut line = Vec::new();
    loop {
        line.clear();
        let size = input.read_until(b'\n', &mut line)?;
        if line.last() != Some(&b'\n') {
            break;
        }
        consumed += size;
        if let Ok(row) = serde_json::from_slice::<serde_json::Value>(&line) {
            insert(&row);
        }
    }
    Ok(consumed)
}
