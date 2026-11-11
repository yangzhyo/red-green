//! 内存诊断不参与输出授权；校验和有界异步记录都在此模块内。
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HeapSample {
    pub free: u32,
    pub largest: u32,
    pub minimum: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemorySample {
    pub uptime_ms: u64,
    pub firmware: String,
    pub sessions: u32,
    pub idle: bool,
    pub screen_on: bool,
    pub internal: HeapSample,
    pub psram: HeapSample,
}
#[derive(Clone, Debug, Serialize)]
pub struct MemoryObservation {
    pub boot: String,
    pub generation: String,
    #[serde(flatten)]
    pub sample: MemorySample,
}
impl MemorySample {
    pub fn parse(value: &Value) -> Option<Self> {
        let sample: Self = serde_json::from_value(value.clone()).ok()?;
        let valid_heap = |h: &HeapSample| {
            h.free <= 64 * 1024 * 1024 && h.largest <= h.free && h.minimum <= h.free
        };
        (sample.uptime_ms <= 9_007_199_254_740_991
            && sample.sessions <= 32
            && !sample.firmware.is_empty()
            && sample.firmware.len() <= 64
            && sample.firmware.bytes().all(|c| c.is_ascii_graphic())
            && valid_heap(&sample.internal)
            && valid_heap(&sample.psram))
        .then_some(sample)
    }
}

use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::mpsc::{sync_channel, SyncSender},
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
#[derive(Serialize)]
struct Record {
    schema: u32,
    received_at_ms: u64,
    #[serde(flatten)]
    observation: MemoryObservation,
}
struct FileLog {
    path: PathBuf,
    limit: u64,
}
impl FileLog {
    fn append(&self, record: &Record) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(record)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "内存样本超过日志上限",
            ));
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let size = match fs::metadata(&self.path) {
            Ok(meta) => meta.len(),
            Err(err) if err.kind() == io::ErrorKind::NotFound => 0,
            Err(err) => return Err(err),
        };
        if size.saturating_add(bytes.len() as u64) > self.limit {
            fs::rename(&self.path, self.path.with_extension("jsonl.1"))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)?;
        let before = file.metadata()?.len();
        if let Err(error) = file.write_all(&bytes) {
            let _ = file.set_len(before);
            return Err(error);
        }
        Ok(())
    }
}
pub struct Recorder {
    sender: SyncSender<Record>,
}
impl Recorder {
    pub fn start() -> io::Result<Self> {
        let path = std::env::var_os("RED_GREEN_MOSAICO_MEMORY_LOG")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| {
                    PathBuf::from(home).join("Library/Logs/red-green-mosaico-memory.jsonl")
                })
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "未找到日志目录"))?;
        let (sender, receiver) = sync_channel(8);
        std::thread::Builder::new()
            .name("mosaico-memory".into())
            .spawn(move || {
                let writer = FileLog {
                    path,
                    limit: MAX_LOG_BYTES,
                };
                for record in receiver {
                    if let Err(error) = writer.append(&record) {
                        eprintln!("mosaico 内存日志写入失败: {error}");
                    }
                }
            })?;
        Ok(Self { sender })
    }
    pub fn submit(&self, observation: MemoryObservation) -> bool {
        let Ok(elapsed) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return false;
        };
        self.sender
            .try_send(Record {
                schema: 1,
                received_at_ms: elapsed.as_millis() as u64,
                observation,
            })
            .is_ok()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn observation() -> MemoryObservation {
        MemoryObservation { boot: "boot1".into(), generation: "g1".into(), sample: MemorySample::parse(&serde_json::json!({
            "uptime_ms":60000,"firmware":"test-build","sessions":0,"idle":true,"screen_on":false,
            "internal":{"free":100000,"largest":50000,"minimum":90000},
            "psram":{"free":3000000,"largest":2500000,"minimum":2900000}
        })).unwrap() }
    }
    fn record(time: u64) -> Record {
        Record {
            schema: 1,
            received_at_ms: time,
            observation: observation(),
        }
    }
    fn directory() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "rg-memory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn records_real_json_and_rotates_at_size_limit() {
        let dir = directory();
        let path = dir.join("samples.jsonl");
        let size = serde_json::to_vec(&record(1000)).unwrap().len() as u64 + 1;
        let writer = FileLog {
            path: path.clone(),
            limit: size * 2,
        };
        writer.append(&record(1000)).unwrap();
        writer.append(&record(2000)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
        writer.append(&record(3000)).unwrap();
        let current: Value =
            serde_json::from_str(fs::read_to_string(&path).unwrap().trim()).unwrap();
        assert_eq!(current["received_at_ms"], 3000);
        assert_eq!(current["boot"], "boot1");
        assert_eq!(current["internal"]["free"], 100000);
        let previous = fs::read_to_string(path.with_extension("jsonl.1")).unwrap();
        assert_eq!(previous.lines().count(), 2);
        assert!(fs::metadata(&path).unwrap().len() <= writer.limit);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn log_failure_preserves_old_records_and_can_recover() {
        let dir = directory();
        let path = dir.join("samples.jsonl");
        let writer = FileLog {
            path: path.clone(),
            limit: 500,
        };
        writer.append(&record(1000)).unwrap();
        fs::create_dir(path.with_extension("jsonl.1")).unwrap();
        assert!(writer.append(&record(2000)).is_err());
        assert!(fs::read_to_string(&path).unwrap().contains("1000"));
        fs::remove_dir(path.with_extension("jsonl.1")).unwrap();
        writer.append(&record(2000)).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("2000"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn full_record_queue_does_not_wait_for_disk() {
        let (sender, receiver) = sync_channel(1);
        let recorder = Recorder { sender };
        assert!(recorder.submit(observation()));
        let (done_tx, done_rx) = sync_channel(1);
        let worker =
            std::thread::spawn(move || done_tx.send(recorder.submit(observation())).unwrap());
        let result = done_rx.recv_timeout(std::time::Duration::from_millis(500));
        drop(receiver);
        worker.join().unwrap();
        assert_eq!(result.unwrap(), false);
    }
}
