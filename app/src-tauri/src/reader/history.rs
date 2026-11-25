use super::live::UNAVAILABLE;
use crate::codex::{model::is_thread_id, observer::project_name, rpc::Rpc};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path};

pub(super) fn thread_id(key: &str) -> Result<&str, u16> {
    key.strip_prefix("codex-")
        .filter(|id| is_thread_id(id))
        .ok_or(404)
}

fn eligible_id(thread: &Value) -> Option<&str> {
    // 共享 App Server 把部分本机 CLI 标为 vscode，originator 保留了终端身份。
    let cli = thread["source"] == "cli"
        || (thread["source"] == "vscode" && thread["originator"] == "codex-tui");
    if !cli
        || thread["ephemeral"] != false
        || !thread["parentThreadId"].is_null()
        || !thread["attachedToThreadId"].is_null()
    {
        return None;
    }
    thread["id"].as_str().filter(|id| is_thread_id(id))
}

fn page(rpc: &mut Rpc, cursor: Option<&str>, id: Option<&str>, fresh: bool) -> Result<Value, u16> {
    // 旧服务可能静默忽略不认识的 JSON 字段。0.171.0 已验证同时支持账号和 ID 筛选。
    if rpc.server_version.is_none_or(|v| v < (0, 171, 0)) {
        return Err(501);
    }
    let mut params = json!({"currentAccountOnly":true, "archived":false, "sourceKinds":["cli","vscode"],
        "cursor":cursor, "sortKey":"updated_at", "sortDirection":"desc", "limit":24});
    if let Some(id) = id {
        params["includedThreadIds"] = json!([id]);
        params["limit"] = 1.into();
    }
    let mut result = rpc.call("thread/list", params.clone());
    if fresh && rpc.account_changed {
        // 初始化时可能收到当前账号通知；在同一连接上重新取得账号范围。
        rpc.account_changed = false;
        result = rpc.call("thread/list", params);
    }
    if rpc.account_changed {
        return Err(409);
    }
    result.map_err(|_| UNAVAILABLE)
}

pub(super) fn find(rpc: &mut Rpc, id: &str, fresh: bool) -> Result<Value, u16> {
    let mut result = page(rpc, None, Some(id), fresh)?;
    result["data"]
        .as_array_mut()
        .ok_or(UNAVAILABLE)?
        .iter_mut()
        .find(|row| eligible_id(row) == Some(id))
        .map(Value::take)
        .ok_or(404)
}

pub(super) fn loaded_threads(rpc: &mut Rpc) -> Result<Vec<String>, u16> {
    let mut ids = Vec::new();
    let mut cursor = None;
    let mut seen = HashSet::new();
    loop {
        let page = rpc
            .call("thread/loaded/list", json!({"limit":100,"cursor":cursor}))
            .map_err(|_| UNAVAILABLE)?;
        if rpc.account_changed {
            return Err(409);
        }
        for id in page["data"].as_array().ok_or(UNAVAILABLE)? {
            ids.push(id.as_str().ok_or(UNAVAILABLE)?.to_owned());
        }
        let Some(next) = page["nextCursor"].as_str() else {
            return Ok(ids);
        };
        if !seen.insert(next.to_owned()) {
            return Err(UNAVAILABLE);
        }
        cursor = Some(next.to_owned());
    }
}

#[derive(Default)]
pub(super) struct Directory {
    active: Option<(String, Rpc)>,
}

impl Directory {
    pub(super) fn list(&mut self, socket: &Path, cursor: Option<&str>) -> Result<Value, u16> {
        let cursor = match cursor {
            Some(token) => {
                let (generation, cursor) = token.split_once(':').ok_or(409u16)?;
                if !self
                    .active
                    .as_ref()
                    .is_some_and(|(key, _)| key == generation)
                {
                    return Err(409);
                }
                Some(cursor)
            }
            None => None,
        };
        let fresh = self.active.is_none();
        if fresh {
            self.active = Some((
                format!("{:032x}", rand::random::<u128>()),
                Rpc::connect(socket).map_err(|_| UNAVAILABLE)?,
            ));
        }
        let (generation, rpc) = self.active.as_mut().unwrap();
        match list_pages(rpc, cursor, fresh) {
            Ok((entries, next)) => Ok(json!({"entries":entries,"generation":generation,
                "nextCursor":next.map(|cursor|format!("{generation}:{cursor}"))})),
            Err(status) => {
                self.active = None;
                Err(if !fresh && status == UNAVAILABLE {
                    409
                } else {
                    status
                })
            }
        }
    }

    pub(super) fn status(&mut self, generation: &str) -> Result<Value, u16> {
        let (_, rpc) = self
            .active
            .as_mut()
            .filter(|(key, _)| key == generation)
            .ok_or(409u16)?;
        if rpc.call("thread/loaded/list", json!({"limit":1})).is_err() || rpc.account_changed {
            self.active = None;
            return Err(409);
        }
        Ok(json!({}))
    }
}

fn list_pages(
    rpc: &mut Rpc,
    cursor: Option<&str>,
    fresh: bool,
) -> Result<(Vec<Value>, Option<String>), u16> {
    let mut cursor = cursor.map(str::to_owned);
    let mut seen = HashSet::new();
    // 桌面客户端的整页都可能被筛掉。限制单个请求的工作量，同时保留下一页。
    for i in 0..4 {
        if !seen.insert(cursor.clone()) {
            return Err(UNAVAILABLE);
        }
        let page = page(rpc, cursor.as_deref(), None, fresh && i == 0)?;
        let entries: Vec<_> = page["data"]
            .as_array()
            .ok_or(UNAVAILABLE)?
            .iter()
            .filter_map(list_entry)
            .collect();
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if !entries.is_empty() || cursor.is_none() {
            return Ok((entries, cursor));
        }
    }
    Ok((vec![], cursor))
}

fn list_entry(row: &Value) -> Option<Value> {
    let id = eligible_id(row)?;
    let title = ["name", "preview"]
        .into_iter()
        .filter_map(|field| row[field].as_str())
        .flat_map(str::lines)
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("未命名会话")
        .chars()
        .take(120)
        .collect::<String>();
    let project = row["cwd"]
        .as_str()
        .filter(|s| Path::new(s).is_absolute())
        .map(project_name)
        .unwrap_or_else(|| "?".into());
    Some(
        json!({"key":format!("codex-{id}"), "source":"Codex CLI", "title":title,
        "project":project,"updatedAt":row["updatedAt"].as_i64()?}),
    )
}
