#!/usr/bin/env node
// red-green: Antigravity CLI 会话状态、账号用量与阅读映射采集模块。
// 由 hooks/antigravity.sh 在 statusLine / title 与极简 hooks（Stop、ask_question）中调用。
// 必须保持快速、静默容错；写入 ~/.claude/session-status/（或 RED_GREEN_STATUS_DIR），app 只读。

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const USAGE_FILE = "antigravity-usage.json";
const TERMINAL_STATES = new Set(["idle", "completed", "your_turn", "aborted"]);
const RUNNING_AGENT_STATES = new Set(["working", "thinking", "tool_use"]);
const INIT_AGENT_STATES = new Set(["authenticating", "initializing"]);
const ABORT_REASONS = new Set([
  "ERROR",
  "MAX_INVOCATIONS",
  "MAX_FORCED_INVOCATIONS",
  "MAX_TOKEN_BUDGET_EXCEEDED",
]);

const QUESTION_PHRASES = [
  "请确认",
  "请选择",
  "请告诉我",
  "要不要",
  "你想",
  "你希望",
  "哪个",
  "哪种",
  "哪项",
  "哪一个",
  "哪一种",
  "哪一项",
];

// 与 Claude Code（hooks/session-status.sh）和 Codex CLI（codex/model.rs）保持同一末行问句启发式。
export function isQuestion(text) {
  if (typeof text !== "string" || !text.trim()) return false;
  const lines = text.split("\n").map((l) => l).filter((l) => l.trim().length > 0);
  const last = lines.at(-1) ?? "";
  const tail = Array.from(last).slice(-240).join("");
  const end = tail.replace(/["*_)）】`\s]+$/u, "");
  if (/[?？]$/u.test(end)) return true;
  return QUESTION_PHRASES.some((phrase) => tail.includes(phrase));
}

export function sliceStart(text, limit) {
  if (typeof text !== "string") return "";
  return Array.from(text).slice(0, limit).join("");
}

export function sliceEnd(text, limit) {
  if (typeof text !== "string") return "";
  return Array.from(text).slice(-limit).join("");
}

export function isCliTranscriptPath(transcriptPath, artifactDir) {
  const paths = [transcriptPath, artifactDir].filter((p) => typeof p === "string" && p.length > 0);
  if (paths.length === 0) return false;
  return paths.some((p) => /(?:^|[\\/])\.gemini[\\/]antigravity-cli(?:[\\/]|$)/.test(p));
}

export function validConversationId(id) {
  return typeof id === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(id);
}

// agy 1.5.18 的 statusLine 将 transcript_path 误指向 ~/.gemini/antigravity/brain/...，
// 实际 CLI 对话记录位于 ~/.gemini/antigravity-cli/brain/<cid>/.system_generated/logs/transcript_full.jsonl。
export function resolveTranscriptPath(cid, reportedPath, homeDir = os.homedir(), exists = fs.existsSync) {
  const candidates = [];
  if (validConversationId(cid)) {
    const cliLogs = path.join(homeDir, ".gemini", "antigravity-cli", "brain", cid, ".system_generated", "logs");
    candidates.push(path.join(cliLogs, "transcript_full.jsonl"));
    candidates.push(path.join(cliLogs, "transcript.jsonl"));
  }
  if (typeof reportedPath === "string" && reportedPath.length > 0) {
    const expanded = reportedPath.startsWith("~/") ? path.join(homeDir, reportedPath.slice(2)) : reportedPath;
    if (!cid || expanded.includes(cid)) {
      if (expanded.includes("/.gemini/antigravity/brain/")) {
        const fixed = expanded.replace("/.gemini/antigravity/brain/", "/.gemini/antigravity-cli/brain/");
        const dir = path.dirname(fixed);
        candidates.push(path.join(dir, "transcript_full.jsonl"), fixed);
      }
      if (path.basename(expanded) === "transcript.jsonl") {
        candidates.push(path.join(path.dirname(expanded), "transcript_full.jsonl"));
      }
      candidates.push(expanded);
    }
  }
  const seen = new Set();
  for (const candidate of candidates) {
    if (seen.has(candidate)) continue;
    seen.add(candidate);
    try {
      if (path.isAbsolute(candidate) && exists(candidate)) return candidate;
    } catch {
      // ignore
    }
  }
  return "";
}

// 仅提取当前回合（最后一条用户提问之后）的最终可见助手答复。
// 跳过带 tool_calls 的中间过渡文本，且若后续又执行了工具（GENERIC），清除旧答复，
// 避免用户在工具阶段按 Esc 取消时把中间过渡句误判为已完成。
export function extractLastAssistantText(jsonlContent) {
  if (typeof jsonlContent !== "string" || !jsonlContent) return "";
  const lines = jsonlContent.split("\n");
  let lastTurnReply = "";
  let seenUser = false;
  let anyTerminalReply = "";
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) continue;
    let row;
    try {
      row = JSON.parse(line);
    } catch {
      continue;
    }
    if (row?.type === "USER_INPUT" && row?.source === "USER_EXPLICIT") {
      seenUser = true;
      lastTurnReply = "";
    } else if (row?.type === "PLANNER_RESPONSE" && row?.source === "MODEL") {
      const hasTools = Array.isArray(row?.tool_calls) && row.tool_calls.length > 0;
      if (hasTools) {
        lastTurnReply = "";
      } else if (typeof row?.content === "string" && row.content.trim().length > 0) {
        lastTurnReply = row.content;
        anyTerminalReply = row.content;
      }
    } else if (row?.type === "GENERIC") {
      lastTurnReply = "";
    }
  }
  return seenUser ? lastTurnReply : anyTerminalReply;
}

function readTranscriptTail(filePath) {
  if (!filePath) return "";
  try {
    const stat = fs.statSync(filePath);
    // 读取尾部最多 512 KiB 足以覆盖最近几轮对话；若文件更小则整文件读取。
    const maxBytes = 512 * 1024;
    if (stat.size <= maxBytes) {
      return fs.readFileSync(filePath, "utf8");
    }
    const fd = fs.openSync(filePath, "r");
    try {
      const buf = Buffer.alloc(maxBytes);
      const bytesRead = fs.readSync(fd, buf, 0, maxBytes, stat.size - maxBytes);
      const text = buf.subarray(0, bytesRead).toString("utf8");
      const firstNewline = text.indexOf("\n");
      return firstNewline >= 0 ? text.slice(firstNewline + 1) : text;
    } finally {
      fs.closeSync(fd);
    }
  } catch {
    return "";
  }
}

function parseBucketKey(bucketId) {
  const suffixes = [
    [/-5h$/i, 300],
    [/-weekly$/i, 10080],
    [/-7d$/i, 10080],
    [/-daily$/i, 1440],
    [/-24h$/i, 1440],
    [/-1d$/i, 1440],
  ];
  for (const [re, minutes] of suffixes) {
    if (re.test(bucketId)) {
      return { prefix: bucketId.replace(re, ""), duration_minutes: minutes };
    }
  }
  const mHour = bucketId.match(/^(.*)-(\d+)h$/i);
  if (mHour) return { prefix: mHour[1], duration_minutes: Number(mHour[2]) * 60 };
  const mDay = bucketId.match(/^(.*)-(\d+)d$/i);
  if (mDay) return { prefix: mDay[1], duration_minutes: Number(mDay[2]) * 1440 };
  const mMin = bucketId.match(/^(.*)-(\d+)m$/i);
  if (mMin) return { prefix: mMin[1], duration_minutes: Number(mMin[2]) };
  return { prefix: bucketId, duration_minutes: null };
}

function groupLabel(prefix) {
  const key = prefix.toLowerCase();
  if (key === "gemini") return "Gemini";
  if (key === "3p") return "Claude / GPT";
  return prefix || "Antigravity";
}

function groupOrder(prefix) {
  const key = prefix.toLowerCase();
  if (key === "gemini") return 0;
  if (key === "3p") return 1;
  return 2;
}

export function opaqueAccountId(email) {
  if (typeof email !== "string" || !email.trim()) return "current";
  const digest = crypto.createHash("sha256").update(email.trim().toLowerCase()).digest("hex").slice(0, 12);
  return `acct-${digest}`;
}

// 将 Antigravity CLI statusLine 的 quota 映射归一化为账号级额度组。
// 对邮箱做单向哈希后再作为 account_id 与组 ID，避免明文邮箱写入状态文件或经 Mosaico 串口外发。
// remaining_fraction=1 且 reset_time="1970-01-01T00:00:00Z" 表示该窗口尚未消耗（真实 0%，暂无重置倒计时）。
export function normalizeQuota(quota, email, planTier, nowSec) {
  const accountId = opaqueAccountId(email);
  const byPrefix = new Map();
  if (quota && typeof quota === "object" && !Array.isArray(quota)) {
    for (const [bucketId, info] of Object.entries(quota)) {
      if (!info || typeof info !== "object" || info.disabled === true) continue;
      const { prefix, duration_minutes } = parseBucketKey(bucketId);
      let usedPercentage = null;
      if (
        typeof info.remaining_fraction === "number" &&
        Number.isFinite(info.remaining_fraction) &&
        info.remaining_fraction >= 0 &&
        info.remaining_fraction <= 1
      ) {
        const raw = (1 - info.remaining_fraction) * 100;
        usedPercentage = Math.min(100, Math.max(0, Math.round(raw * 100) / 100));
      }
      let resetsAt = null;
      if (typeof info.reset_time === "string" && info.reset_time.trim()) {
        const ms = Date.parse(info.reset_time);
        if (Number.isFinite(ms)) {
          if (ms > 0) resetsAt = Math.floor(ms / 1000);
          // ms === 0（1970-01-01T00:00:00Z）表示窗口未启动，保留 resetsAt = null。
        }
      } else if (
        typeof info.reset_in_seconds === "number" &&
        Number.isFinite(info.reset_in_seconds) &&
        info.reset_in_seconds > 0
      ) {
        resetsAt = nowSec + Math.floor(info.reset_in_seconds);
      }
      if (!byPrefix.has(prefix)) byPrefix.set(prefix, []);
      byPrefix.get(prefix).push({
        key: bucketId,
        used_percentage: usedPercentage,
        duration_minutes,
        resets_at: resetsAt,
      });
    }
  }
  const groups = [...byPrefix.entries()]
    .sort(([a], [b]) => groupOrder(a) - groupOrder(b) || a.localeCompare(b))
    .map(([prefix, windows]) => {
      windows.sort(
        (a, b) =>
          (a.duration_minutes ?? Number.MAX_SAFE_INTEGER) -
            (b.duration_minutes ?? Number.MAX_SAFE_INTEGER) || a.key.localeCompare(b.key)
      );
      return {
        id: `antigravity:${accountId}:${prefix.toLowerCase()}`,
        source: "antigravity",
        account_id: accountId,
        label: groupLabel(prefix),
        windows,
      };
    });
  return {
    updated_at: nowSec,
    account_id: accountId,
    ...(typeof planTier === "string" && planTier.trim() ? { plan_tier: planTier.trim() } : {}),
    groups,
  };
}

function normalizeTitle(title) {
  if (typeof title !== "string") return "";
  const first = title
    .split("\n")
    .map((l) => l.trim())
    .find((l) => l.length > 0);
  return first ? sliceStart(first, 120) : "";
}

export function nextSessionState({
  kind,
  payload,
  prev,
  transcriptText = "",
  nowIso = new Date().toISOString(),
}) {
  // statusLine 中显式传入 conversation_id: ""（如执行 /clear 后）必须按清空处理，不能回退到旧会话 ID。
  const hasExplicitStatusCid =
    kind === "statusline" &&
    (typeof payload?.conversation_id === "string" || typeof payload?.session_id === "string");
  const cid = hasExplicitStatusCid
    ? (typeof payload?.conversation_id === "string" && payload.conversation_id) ||
      (typeof payload?.session_id === "string" && payload.session_id) ||
      ""
    : (typeof payload?.conversation_id === "string" && payload.conversation_id) ||
      (typeof payload?.conversationId === "string" && payload.conversationId) ||
      (typeof payload?.session_id === "string" && payload.session_id) ||
      prev?.conversation_id ||
      "";
  const sameConv = !prev || !prev.conversation_id || prev.conversation_id === cid;
  const prevState = sameConv ? prev?.state : null;
  const prevEvent = sameConv ? prev?.event : null;
  const prevSince = sameConv ? prev?.since : null;
  const prevDetail = sameConv ? prev?.detail ?? "" : "";
  const reportedTitle = normalizeTitle(payload?.conversation_title);
  const title = reportedTitle || (sameConv ? prev?.title ?? "" : "");

  if (kind === "pretool") {
    const toolName = String(payload?.toolCall?.name ?? "").toLowerCase();
    if (toolName !== "ask_question") return null;
    const firstQ = payload?.toolCall?.args?.questions?.[0]?.question ?? "";
    return {
      conversation_id: cid,
      title,
      state: "awaiting",
      detail: sliceStart(typeof firstQ === "string" ? firstQ : "", 300),
      since: nowIso,
      event: "PreToolUse:ask_question",
    };
  }

  if (kind === "posttool") {
    const toolName = String(payload?.toolCall?.name ?? "").toLowerCase();
    if (toolName !== "ask_question") return null;
    if (prevState !== "awaiting" || prevEvent !== "PreToolUse:ask_question") return null;
    return {
      conversation_id: cid,
      title,
      state: "running",
      detail: "",
      since: nowIso,
      event: "PostToolUse:ask_question",
    };
  }

  if (kind === "stop") {
    const reason = String(payload?.terminationReason ?? "").trim();
    const errText = typeof payload?.error === "string" ? payload.error.trim() : "";
    let state;
    let detail = "";
    let event = `Stop:${reason || "NO_TOOL_CALL"}`;
    if (reason === "USER_CANCELED") {
      state = "idle";
    } else if (ABORT_REASONS.has(reason) || errText.length > 0) {
      state = "aborted";
      detail = sliceStart(errText || reason, 300);
      event = `Stop:${reason || "ERROR"}`;
    } else {
      const reply = extractLastAssistantText(transcriptText);
      if (isQuestion(reply)) {
        state = "your_turn";
        detail = sliceEnd(reply, 300);
      } else {
        state = "completed";
        detail = sliceStart(reply, 300);
      }
    }
    // 若 statusLine 先一步完成兜底分类且结果一致，沿用同一 since，避免重置已阅或重复叫声。
    const keepSince = prevEvent === "statusLine:idle-fallback" && prevState === state && prevSince;
    return {
      conversation_id: cid,
      title,
      state,
      detail,
      since: keepSince ? prevSince : nowIso,
      event,
    };
  }

  // statusLine / title 事件
  const agentState = String(payload?.agent_state ?? "idle").toLowerCase();
  const confirmPending = payload?.tool_confirmation_pending === true;
  if (confirmPending) {
    const unchanged = prevState === "awaiting" && prevEvent === "statusLine:confirm";
    return {
      conversation_id: cid,
      title,
      state: "awaiting",
      detail: unchanged ? prevDetail : "",
      since: unchanged && prevSince ? prevSince : nowIso,
      event: "statusLine:confirm",
    };
  }
  if (agentState === "reviewing") {
    const unchanged = prevState === "awaiting" && prevEvent === "statusLine:reviewing";
    return {
      conversation_id: cid,
      title,
      state: "awaiting",
      detail: unchanged ? prevDetail : "",
      since: unchanged && prevSince ? prevSince : nowIso,
      event: "statusLine:reviewing",
    };
  }
  if (agentState === "error") {
    const unchanged = prevState === "aborted";
    return {
      conversation_id: cid,
      title,
      state: "aborted",
      detail: unchanged ? prevDetail : "",
      since: unchanged && prevSince ? prevSince : nowIso,
      event: unchanged && prevEvent ? prevEvent : "statusLine:error",
    };
  }
  if (RUNNING_AGENT_STATES.has(agentState)) {
    // ask_question 等待用户选择期间，statusLine 仍报告 tool_use 且无 tool_confirmation_pending；保留待确认。
    if (prevState === "awaiting" && prevEvent === "PreToolUse:ask_question" && agentState === "tool_use") {
      return {
        conversation_id: cid,
        title,
        state: "awaiting",
        detail: prevDetail,
        since: prevSince || nowIso,
        event: prevEvent,
      };
    }
    const unchanged = prevState === "running";
    return {
      conversation_id: cid,
      title,
      state: "running",
      detail: "",
      since: unchanged && prevSince ? prevSince : nowIso,
      event: `statusLine:${agentState}`,
    };
  }
  if (INIT_AGENT_STATES.has(agentState)) {
    const unchanged = prevState === "idle";
    return {
      conversation_id: cid,
      title,
      state: "idle",
      detail: "",
      since: unchanged && prevSince ? prevSince : nowIso,
      event: `statusLine:${agentState}`,
    };
  }
  // agent_state === "idle"（或其他未知状态）
  if (!prev || !sameConv || !cid) {
    return {
      conversation_id: cid,
      title,
      state: "idle",
      detail: "",
      since: prevState === "idle" && prevSince ? prevSince : nowIso,
      event: "statusLine:idle",
    };
  }
  if (TERMINAL_STATES.has(prevState)) {
    return {
      conversation_id: cid,
      title,
      state: prevState,
      detail: prevDetail,
      since: prevSince || nowIso,
      event: prevEvent || "statusLine:idle",
    };
  }
  // 从 running / awaiting 直接回到 idle（如未装 hooks 或 statusLine 先到）：读对话记录尾部兜底判定。
  const reply = extractLastAssistantText(transcriptText);
  if (!reply) {
    return {
      conversation_id: cid,
      title,
      state: "idle",
      detail: "",
      since: nowIso,
      event: "statusLine:idle",
    };
  }
  const question = isQuestion(reply);
  return {
    conversation_id: cid,
    title,
    state: question ? "your_turn" : "completed",
    detail: question ? sliceEnd(reply, 300) : sliceStart(reply, 300),
    since: nowIso,
    event: "statusLine:idle-fallback",
  };
}

function statusDir() {
  return process.env.RED_GREEN_STATUS_DIR || path.join(os.homedir(), ".claude", "session-status");
}

function discoverProcessContext() {
  const envPid = process.env.RED_GREEN_AGY_PID ? Number(process.env.RED_GREEN_AGY_PID) : null;
  const envTty = process.env.RED_GREEN_AGY_TTY ?? null;
  if (envPid && Number.isInteger(envPid) && envPid > 0) {
    return { agyPid: envPid, tty: envTty ?? "" };
  }
  let pid = process.pid;
  let tty = "";
  let agyPid = null;
  for (let i = 0; i < 10; i++) {
    let out = "";
    try {
      out = execFileSync("ps", ["-o", "ppid=,tty=,comm=", "-p", String(pid)], {
        encoding: "utf8",
        stdio: ["ignore", "pipe", "ignore"],
      }).trim();
    } catch {
      break;
    }
    if (!out) break;
    const match = out.match(/^(\d+)\s+(\S+)\s+(.+)$/);
    if (!match) break;
    const ppid = Number(match[1]);
    const t = match[2];
    const comm = path.basename(match[3].trim());
    if (!tty && t && t !== "??") {
      tty = `/dev/${t}`;
    }
    if (!agyPid && (comm === "agy" || comm === "agy-cli")) {
      agyPid = pid;
      if (t && t !== "??") tty = `/dev/${t}`;
      break;
    }
    if (!ppid || ppid <= 1) break;
    pid = ppid;
  }
  return { agyPid, tty };
}

function projectName(cwd) {
  if (!cwd) return "?";
  try {
    const root = execFileSync("git", ["-C", cwd, "rev-parse", "--show-toplevel"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    if (root) return path.basename(root) || "?";
  } catch {
    // not a git repo
  }
  return path.basename(cwd) || "?";
}

function atomicWrite(filePath, content, mode = 0o600) {
  const dir = path.dirname(filePath);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = `${filePath}.${process.pid}.${Math.random().toString(36).slice(2, 8)}.tmp`;
  fs.writeFileSync(tmp, content, { mode });
  fs.renameSync(tmp, filePath);
}

function writeReaderMapping(dir, cid, value) {
  if (!validConversationId(cid) || !value) return;
  const readerDir = path.join(dir, ".reader");
  const mappingFile = path.join(readerDir, `antigravity-${cid}`);
  try {
    if (fs.existsSync(mappingFile) && fs.readFileSync(mappingFile, "utf8").trim() === value) {
      return;
    }
  } catch {
    // overwrite below
  }
  fs.mkdirSync(readerDir, { recursive: true, mode: 0o700 });
  atomicWrite(mappingFile, `${value}\n`, 0o600);
}

function withFileLock(lockDir, fn) {
  let acquired = false;
  for (let i = 0; i < 25; i++) {
    try {
      fs.mkdirSync(lockDir);
      acquired = true;
      break;
    } catch (e) {
      if (e?.code !== "EEXIST") break;
      try {
        const st = fs.statSync(lockDir);
        if (Date.now() - st.mtimeMs > 2000) {
          fs.rmSync(lockDir, { recursive: true, force: true });
          continue;
        }
      } catch {
        continue;
      }
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 10);
    }
  }
  try {
    return fn();
  } finally {
    if (acquired) {
      try {
        fs.rmSync(lockDir, { recursive: true, force: true });
      } catch {
        // ignore
      }
    }
  }
}

function updateUsage(dir, payload, nowSec) {
  const file = path.join(dir, USAGE_FILE);
  let existing = null;
  try {
    existing = JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    existing = null;
  }
  const hasQuota = payload?.quota && typeof payload.quota === "object" && Object.keys(payload.quota).length > 0;
  const agentState = String(payload?.agent_state ?? "").toLowerCase();
  const accountId = opaqueAccountId(payload?.email);

  if (!hasQuota) {
    // 刚启动或认证中尚未拉取到 quota 时不覆盖同账号缓存，避免新开第二个 agy 窗口时用量表闪现未知；
    // 若账号已切换/登出（accountId 变化），或进入实际会话后仍缺席 quota，则清为空分组以展示未知。
    if (!INIT_AGENT_STATES.has(agentState) && existing && Array.isArray(existing.groups) && existing.groups.length > 0) {
      const accountChanged = existing.account_id !== accountId;
      const inConversation =
        Boolean(payload?.conversation_id || payload?.session_id) ||
        RUNNING_AGENT_STATES.has(agentState);
      if (accountChanged || inConversation) {
        const cleared = { updated_at: nowSec, account_id: accountId, groups: [] };
        atomicWrite(file, `${JSON.stringify(cleared)}\n`, 0o600);
      }
    }
    return;
  }
  const next = normalizeQuota(payload.quota, payload?.email, payload?.plan_tier, nowSec);
  const stripTime = (obj) => (obj ? JSON.stringify({ ...obj, updated_at: undefined }) : "");
  if (stripTime(existing) === stripTime(next)) return;
  atomicWrite(file, `${JSON.stringify(next)}\n`, 0o600);
}

function pruneDeadSessions(dir, currentPid, activeCid) {
  let entries;
  try {
    entries = fs.readdirSync(dir);
  } catch {
    return;
  }
  const dead = [];
  const liveCids = new Set();
  if (activeCid) liveCids.add(activeCid);
  for (const name of entries) {
    const m = name.match(/^antigravity-pid-(\d+)\.json$/);
    if (!m) continue;
    const pid = Number(m[1]);
    if (!pid) continue;
    const full = path.join(dir, name);
    let data = null;
    try {
      data = JSON.parse(fs.readFileSync(full, "utf8"));
    } catch {
      data = null;
    }
    if (pid === currentPid) {
      if (data?.conversation_id) liveCids.add(data.conversation_id);
      continue;
    }
    let alive = true;
    try {
      process.kill(pid, 0);
    } catch (e) {
      if (e?.code === "ESRCH") alive = false;
    }
    if (alive) {
      if (data?.conversation_id) liveCids.add(data.conversation_id);
    } else {
      dead.push({ full, cid: data?.conversation_id || "" });
    }
  }
  for (const item of dead) {
    // 若新进程通过 --resume 接续了同一 conversation_id，不得把活映射覆写为 ended。
    if (item.cid && !liveCids.has(item.cid)) {
      writeReaderMapping(dir, item.cid, "ended");
    }
    try {
      fs.rmSync(item.full, { force: true });
    } catch {
      // ignore
    }
  }
}

function findSessionFileByConversation(dir, cid) {
  if (!validConversationId(cid)) return null;
  let entries;
  try {
    entries = fs.readdirSync(dir);
  } catch {
    return null;
  }
  for (const name of entries) {
    if (!name.startsWith("antigravity-") || !name.endsWith(".json") || name === USAGE_FILE) continue;
    const full = path.join(dir, name);
    try {
      const data = JSON.parse(fs.readFileSync(full, "utf8"));
      if (data?.source === "antigravity" && data?.conversation_id === cid) {
        return { file: full, pid: data.pid ?? null };
      }
    } catch {
      // ignore
    }
  }
  return null;
}

function appendEventLog(dir, line) {
  const log = path.join(dir, ".events.log");
  try {
    fs.appendFileSync(log, `${line}\n`);
    const lines = fs.readFileSync(log, "utf8").split("\n");
    if (lines.length > 2001) {
      atomicWrite(log, `${lines.slice(-1001).join("\n")}`, 0o644);
    }
  } catch {
    // ignore
  }
}

export function handleEvent(kind, payload, opts = {}) {
  const dir = opts.dir ?? statusDir();
  const homeDir = opts.homeDir ?? os.homedir();
  const now = opts.now ?? new Date();
  const nowIso = now.toISOString();
  const nowSec = Math.floor(now.getTime() / 1000);
  fs.mkdirSync(dir, { recursive: true });

  const isHook = kind === "pretool" || kind === "posttool" || kind === "stop";
  if (isHook) {
    const allowAny = opts.allowAnyPath ?? process.env.RED_GREEN_AGY_ALLOW_ANY_PATH === "1";
    if (!allowAny && !isCliTranscriptPath(payload?.transcriptPath, payload?.artifactDirectoryPath)) {
      return { written: false, reason: "non-cli" };
    }
  }

  const ctx = opts.processContext ?? discoverProcessContext();
  if (kind === "statusline") {
    updateUsage(dir, payload, nowSec);
  }

  const cid =
    (typeof payload?.conversation_id === "string" && payload.conversation_id) ||
    (typeof payload?.conversationId === "string" && payload.conversationId) ||
    (typeof payload?.session_id === "string" && payload.session_id) ||
    "";

  let agyPid = ctx.agyPid;
  let tty = ctx.tty || "";
  let file = agyPid ? path.join(dir, `antigravity-pid-${agyPid}.json`) : null;

  if (isHook) {
    if (!file || (!tty && !fs.existsSync(file))) {
      const existing = findSessionFileByConversation(dir, cid);
      if (!existing) return { written: false, reason: "headless-or-untracked" };
      file = existing.file;
      agyPid = existing.pid;
    }
  } else if (!file) {
    return { written: false, reason: "no-agy-pid" };
  }

  // 锁目录放在临时目录而非被 watch 的 statusDir，避免 statusLine 每次未变化重绘触发 Rust watcher。
  const dirKey = crypto.createHash("sha256").update(path.resolve(dir)).digest("hex").slice(0, 12);
  const lockDir = path.join(os.tmpdir(), `red-green-agy-lock-${dirKey}-${agyPid ?? "shared"}`);
  return withFileLock(lockDir, () => {
    let prev = null;
    try {
      prev = JSON.parse(fs.readFileSync(file, "utf8"));
    } catch {
      prev = null;
    }

    const hasExplicitStatusCid =
      kind === "statusline" &&
      (typeof payload?.conversation_id === "string" || typeof payload?.session_id === "string");
    const effectiveCid = hasExplicitStatusCid ? cid : cid || prev?.conversation_id || "";
    const reportedTranscript = payload?.transcriptPath || payload?.transcript_path || "";
    const resolvedTranscript = resolveTranscriptPath(effectiveCid, reportedTranscript, homeDir);

    if (prev?.conversation_id && prev.conversation_id !== effectiveCid) {
      writeReaderMapping(dir, prev.conversation_id, "ended");
    }
    if (validConversationId(effectiveCid) && resolvedTranscript) {
      writeReaderMapping(dir, effectiveCid, resolvedTranscript);
    }

    const needsTranscript =
      kind === "stop" ||
      (kind === "statusline" &&
        String(payload?.agent_state ?? "idle").toLowerCase() === "idle" &&
        prev &&
        (!prev.conversation_id || prev.conversation_id === effectiveCid) &&
        !TERMINAL_STATES.has(prev.state));
    const transcriptText = needsTranscript ? readTranscriptTail(resolvedTranscript) : "";

    const transition = nextSessionState({ kind, payload, prev, transcriptText, nowIso });
    if (!transition) return { written: false, reason: "ignored" };

    const cwd =
      payload?.workspace?.project_dir ||
      payload?.cwd ||
      payload?.workspace?.current_dir ||
      (Array.isArray(payload?.workspacePaths) ? payload.workspacePaths[0] : "") ||
      prev?.cwd ||
      "";
    const project =
      prev?.project && prev.project !== "?"
        ? prev.project
        : projectName(cwd);
    const effectiveTty = tty || prev?.tty || "";
    const sessionId = prev?.session_id || (agyPid ? `pid-${agyPid}` : `cid-${effectiveCid}`);

    const next = {
      session_id: sessionId,
      source: "antigravity",
      ...(agyPid ? { pid: agyPid } : {}),
      conversation_id: transition.conversation_id,
      ...(transition.title ? { title: transition.title } : {}),
      state: transition.state,
      cwd,
      project,
      tty: effectiveTty,
      detail: transition.detail,
      since: transition.since,
      event: transition.event,
    };

    // 状态与展示元信息都没变时不重写文件，避免频繁触发 watcher。
    const sameSnapshot =
      prev &&
      prev.state === next.state &&
      prev.since === next.since &&
      prev.conversation_id === next.conversation_id &&
      (prev.title ?? "") === (next.title ?? "") &&
      prev.tty === next.tty &&
      prev.cwd === next.cwd &&
      prev.project === next.project &&
      prev.detail === next.detail &&
      prev.event === next.event;
    if (sameSnapshot) {
      return { written: false, reason: "unchanged", session: next };
    }

    const stateChanged = !prev || prev.state !== next.state;
    atomicWrite(file, `${JSON.stringify(next)}\n`, 0o600);
    if (stateChanged) {
      appendEventLog(dir, `${nowIso} agy:${agyPid ?? sessionId} ${next.event} -> ${next.state}`);
    }
    if (!prev && agyPid) {
      pruneDeadSessions(dir, agyPid, transition.conversation_id);
    }
    return { written: true, session: next };
  });
}

function runCli() {
  const mode = process.argv[2] || "statusline";
  const sub = process.argv[3] || "";
  const kind = mode === "hook" ? sub.toLowerCase() : "statusline";
  const hookStdout =
    mode === "hook"
      ? kind === "posttool"
        ? "{}\n"
        : '{"decision":"allow"}\n'
      : "";

  try {
    const raw = fs.readFileSync(0, "utf8");
    const payload = raw.trim() ? JSON.parse(raw) : {};
    const inferredKind =
      mode === "hook" && !kind
        ? {
            PreToolUse: "pretool",
            PostToolUse: "posttool",
            Stop: "stop",
          }[payload.hookEventName] ?? ""
        : kind;
    if (inferredKind) {
      handleEvent(inferredKind, payload);
    }
  } catch {
    // fail-silent: 绝不阻塞或打断 Antigravity CLI
  }
  if (hookStdout) {
    process.stdout.write(hookStdout);
  }
}

const isMain = (() => {
  if (!process.argv[1]) return false;
  try {
    return fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
})();

if (isMain) {
  runCli();
}
