import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  extractLastAssistantText,
  handleEvent,
  isCliTranscriptPath,
  isQuestion,
  normalizeQuota,
  opaqueAccountId,
  resolveTranscriptPath,
} from "../../hooks/antigravity-state.mjs";
import { mergeAntigravitySettings } from "../../scripts/merge-antigravity-settings.mjs";

function makeTempDir(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

test("问句启发式与 Claude / Codex 保持一致，仅看最后非空行", () => {
  assert.equal(isQuestion("正文分析中有问题？\n已经全部完成。"), false);
  assert.equal(isQuestion("已经检查完毕。\n要继续吗？**"), true);
  assert.equal(isQuestion("请确认下一步方案。"), true);
  assert.equal(isQuestion("Ready to proceed?`"), true);
  assert.equal(isQuestion(""), false);
});

test("resolveTranscriptPath 修正 agy statusLine 误报的 antigravity 路径并优先 transcript_full.jsonl", () => {
  const home = makeTempDir("rg-agy-home-");
  try {
    const cid = "7b8f1f5e-b657-4dc6-b177-8be6f20e235c";
    const logsDir = path.join(home, ".gemini", "antigravity-cli", "brain", cid, ".system_generated", "logs");
    fs.mkdirSync(logsDir, { recursive: true });
    const compact = path.join(logsDir, "transcript.jsonl");
    const full = path.join(logsDir, "transcript_full.jsonl");
    fs.writeFileSync(compact, "{}\n");
    const wrongReported = path.join(home, ".gemini", "antigravity", "brain", cid, ".system_generated", "logs", "transcript.jsonl");
    assert.equal(resolveTranscriptPath(cid, wrongReported, home), compact);
    fs.writeFileSync(full, "{}\n");
    assert.equal(resolveTranscriptPath(cid, wrongReported, home), full);
    assert.equal(resolveTranscriptPath("", path.join(home, ".gemini", "antigravity", "brain", ".system_generated", "logs", "transcript.jsonl"), home), "");
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test("normalizeQuota 区分额度组、保留未使用桶的真实零值、跳过禁用桶", () => {
  const normalized = normalizeQuota(
    {
      "3p-weekly": { remaining_fraction: 1, reset_time: "1970-01-01T00:00:00Z", reset_in_seconds: 0 },
      "3p-5h": { remaining_fraction: 1, reset_time: "1970-01-01T00:00:00Z", reset_in_seconds: 0 },
      "gemini-weekly": { remaining_fraction: 0.8893, reset_time: "2026-12-04T02:54:07Z", reset_in_seconds: 513478 },
      "gemini-5h": { remaining_fraction: 0.8487, reset_time: "2026-11-28T08:28:49Z", reset_in_seconds: 15160 },
      "disabled-5h": { remaining_fraction: 0, disabled: true },
    },
    "dev@example.com",
    "Google AI Pro",
    1795753000
  );
  assert.equal(normalized.account_id, opaqueAccountId("dev@example.com"));
  assert.match(normalized.account_id, /^acct-[0-9a-f]{12}$/);
  assert.equal(JSON.stringify(normalized).includes("dev@example.com"), false);
  assert.equal(normalized.plan_tier, "Google AI Pro");
  assert.equal(normalized.groups.length, 2);
  assert.equal(normalized.groups[0].label, "Gemini");
  assert.deepEqual(normalized.groups[0].windows.map((w) => w.key), ["gemini-5h", "gemini-weekly"]);
  assert.equal(normalized.groups[0].windows[0].used_percentage, 15.13);
  assert.equal(normalized.groups[0].windows[0].duration_minutes, 300);
  assert.equal(normalized.groups[0].windows[0].resets_at, Math.floor(Date.parse("2026-11-28T08:28:49Z") / 1000));
  assert.equal(normalized.groups[1].label, "Claude / GPT");
  assert.equal(normalized.groups[1].windows[0].used_percentage, 0);
  assert.equal(normalized.groups[1].windows[0].resets_at, null);
});

test("六种状态转换、ask_question 保护、Stop/statusLine 乱序竞态与切会话标记 ended", () => {
  const dir = makeTempDir("rg-agy-status-");
  const home = makeTempDir("rg-agy-home2-");
  try {
    const cid1 = "11111111-1111-4111-8111-111111111111";
    const cid2 = "22222222-2222-4222-8222-222222222222";
    const logs1 = path.join(home, ".gemini", "antigravity-cli", "brain", cid1, ".system_generated", "logs");
    const logs2 = path.join(home, ".gemini", "antigravity-cli", "brain", cid2, ".system_generated", "logs");
    fs.mkdirSync(logs1, { recursive: true });
    fs.mkdirSync(logs2, { recursive: true });
    const t1 = path.join(logs1, "transcript_full.jsonl");
    const t2 = path.join(logs2, "transcript_full.jsonl");
    fs.writeFileSync(
      t1,
      [
        JSON.stringify({ step_index: 0, source: "USER_EXPLICIT", type: "USER_INPUT", content: "<USER_REQUEST>\n检查状态\n</USER_REQUEST>" }),
        JSON.stringify({ step_index: 1, source: "MODEL", type: "PLANNER_RESPONSE", content: "检查完成，是否继续？" }),
      ].join("\n") + "\n"
    );
    fs.writeFileSync(
      t2,
      [
        JSON.stringify({ step_index: 0, source: "USER_EXPLICIT", type: "USER_INPUT", content: "<USER_REQUEST>\n跑测试\n</USER_REQUEST>" }),
        JSON.stringify({ step_index: 1, source: "MODEL", type: "PLANNER_RESPONSE", content: "全部测试已通过。" }),
      ].join("\n") + "\n"
    );

    const ctx = { agyPid: 4242, tty: "/dev/ttys008" };
    const readSession = () => JSON.parse(fs.readFileSync(path.join(dir, "antigravity-pid-4242.json"), "utf8"));
    let t = 1_795_753_000_000;
    const step = (kind, payload) =>
      handleEvent(kind, payload, { dir, homeDir: home, processContext: ctx, now: new Date((t += 1000)) });

    // 1. 启动前无 conversation_id -> 空闲
    step("statusline", {
      conversation_id: "",
      agent_state: "idle",
      cwd: "/tmp/demo-project",
      email: "a@example.com",
      quota: { "gemini-5h": { remaining_fraction: 0.8, reset_time: "2026-11-28T08:28:49Z" } },
    });
    assert.equal(readSession().state, "idle");
    assert.equal(readSession().project, "demo-project");
    assert.equal(readSession().tty, "/dev/ttys008");
    const usage = JSON.parse(fs.readFileSync(path.join(dir, "antigravity-usage.json"), "utf8"));
    assert.equal(usage.groups[0].windows[0].used_percentage, 20);

    // 2. 开始工作 -> 运行中，记录阅读映射
    step("statusline", { conversation_id: cid1, agent_state: "working", cwd: "/tmp/demo-project", conversation_title: "调研任务" });
    assert.equal(readSession().state, "running");
    assert.equal(readSession().title, "调研任务");
    assert.equal(fs.readFileSync(path.join(dir, ".reader", `antigravity-${cid1}`), "utf8").trim(), t1);

    // 3. 权限弹窗 -> 待确认
    step("statusline", { conversation_id: cid1, agent_state: "tool_use", tool_confirmation_pending: true });
    assert.equal(readSession().state, "awaiting");

    // 4. 恢复运行 -> 运行中
    step("statusline", { conversation_id: cid1, agent_state: "working" });
    assert.equal(readSession().state, "running");

    // 5. ask_question 钩子 -> 待确认，且紧随的 statusLine tool_use 不得覆盖
    step("pretool", {
      conversationId: cid1,
      transcriptPath: t1,
      toolCall: { name: "ask_question", args: { questions: [{ question: "选哪个方案？" }] } },
    });
    assert.equal(readSession().state, "awaiting");
    assert.equal(readSession().detail, "选哪个方案？");
    step("statusline", { conversation_id: cid1, agent_state: "tool_use" });
    assert.equal(readSession().state, "awaiting");
    step("posttool", { conversationId: cid1, transcriptPath: t1, toolCall: { name: "ask_question" } });
    assert.equal(readSession().state, "running");

    // 6. Stop 先到 -> 轮到你；随后的 statusLine idle 不覆盖终态与 since
    step("stop", { conversationId: cid1, transcriptPath: t1, terminationReason: "NO_TOOL_CALL", error: "" });
    const afterStop = readSession();
    assert.equal(afterStop.state, "your_turn");
    step("statusline", { conversation_id: cid1, agent_state: "idle" });
    assert.equal(readSession().state, "your_turn");
    assert.equal(readSession().since, afterStop.since);

    // 7. 同终端切换到 cid2：旧会话阅读映射标为 ended，测试 statusLine:idle 先于 Stop 到达的去重
    step("statusline", { conversation_id: cid2, agent_state: "working", cwd: "/tmp/other-dir" });
    assert.equal(fs.readFileSync(path.join(dir, ".reader", `antigravity-${cid1}`), "utf8").trim(), "ended");
    // 项目名首见锚定，不随后续 cwd 漂移
    assert.equal(readSession().project, "demo-project");
    step("statusline", { conversation_id: cid2, agent_state: "idle" });
    const fallback = readSession();
    assert.equal(fallback.state, "completed");
    step("stop", { conversationId: cid2, transcriptPath: t2, terminationReason: "NO_TOOL_CALL", error: "" });
    assert.equal(readSession().state, "completed");
    assert.equal(readSession().since, fallback.since);

    // 8. 用户取消 -> 空闲；外部错误 -> 异常中止
    step("statusline", { conversation_id: cid2, agent_state: "working" });
    step("stop", { conversationId: cid2, transcriptPath: t2, terminationReason: "USER_CANCELED", error: "" });
    assert.equal(readSession().state, "idle");
    step("statusline", { conversation_id: cid2, agent_state: "working" });
    step("stop", { conversationId: cid2, transcriptPath: t2, terminationReason: "ERROR", error: "quota exceeded" });
    assert.equal(readSession().state, "aborted");
    assert.equal(readSession().detail, "quota exceeded");

    // 9. 非 CLI（Antigravity App / IDE）钩子直接忽略；无 tty 的 headless -p 钩子直接忽略
    assert.equal(isCliTranscriptPath("/Users/u/.gemini/antigravity/brain/x/transcript.jsonl", ""), false);
    assert.equal(isCliTranscriptPath("/Users/u/.gemini/antigravity-ide/brain/x/transcript.jsonl", ""), false);
    const ignoredApp = handleEvent(
      "stop",
      { conversationId: "app-1", transcriptPath: "/Users/u/.gemini/antigravity/brain/app-1/transcript.jsonl", terminationReason: "NO_TOOL_CALL" },
      { dir, homeDir: home, processContext: ctx }
    );
    assert.equal(ignoredApp.written, false);
    const ignoredHeadless = handleEvent(
      "stop",
      { conversationId: "headless-1", transcriptPath: path.join(home, ".gemini/antigravity-cli/brain/headless-1/transcript_full.jsonl"), terminationReason: "NO_TOOL_CALL" },
      { dir, homeDir: home, processContext: { agyPid: 9999, tty: "" } }
    );
    assert.equal(ignoredHeadless.written, false);
    assert.equal(fs.existsSync(path.join(dir, "antigravity-pid-9999.json")), false);

    // 10. 仅取当前回合末条可见答复，忽略旧回合答复、带 tool_calls 的过渡句与工具输出前文本
    assert.equal(
      extractLastAssistantText(
        [
          JSON.stringify({ type: "USER_INPUT", source: "USER_EXPLICIT", content: "<USER_REQUEST>旧问题</USER_REQUEST>" }),
          JSON.stringify({ type: "PLANNER_RESPONSE", source: "MODEL", content: "旧答复？" }),
          JSON.stringify({ type: "USER_INPUT", source: "USER_EXPLICIT", content: "<USER_REQUEST>新问题</USER_REQUEST>" }),
        ].join("\n")
      ),
      ""
    );
    assert.equal(
      extractLastAssistantText(
        [
          JSON.stringify({ type: "USER_INPUT", source: "USER_EXPLICIT", content: "<USER_REQUEST>查一下</USER_REQUEST>" }),
          JSON.stringify({
            type: "PLANNER_RESPONSE",
            source: "MODEL",
            content: "我先运行检查命令。",
            tool_calls: [{ name: "run_command" }],
          }),
          JSON.stringify({ type: "GENERIC", source: "MODEL", content: "tool output" }),
        ].join("\n")
      ),
      ""
    );

    // 11. 锁目录不落在被 watch 的 statusDir 内
    assert.equal(
      fs.readdirSync(dir).some((name) => name.startsWith(".agy-lock-")),
      false
    );

    // 12. /clear（显式空 conversation_id）清空旧终态与会话名，并把旧阅读映射标为 ended
    step("statusline", { conversation_id: cid2, agent_state: "working", conversation_title: "旧任务" });
    step("stop", { conversationId: cid2, transcriptPath: t2, terminationReason: "NO_TOOL_CALL", error: "" });
    assert.equal(readSession().state, "completed");
    step("statusline", { conversation_id: "", agent_state: "idle" });
    assert.equal(readSession().state, "idle");
    assert.equal(readSession().conversation_id, "");
    assert.equal(readSession().title, undefined);
    assert.equal(fs.readFileSync(path.join(dir, ".reader", `antigravity-${cid2}`), "utf8").trim(), "ended");

    // 13. 同账号启动初期未拉到 quota 时不闪烁清缓存；账号切换/登出或会话进行中缺席 quota 时清为空分组
    step("statusline", {
      conversation_id: "",
      agent_state: "idle",
      email: "a@example.com",
      quota: { "gemini-5h": { remaining_fraction: 0.5, reset_time: "2026-11-28T08:28:49Z" } },
    });
    assert.equal(JSON.parse(fs.readFileSync(path.join(dir, "antigravity-usage.json"), "utf8")).groups.length, 1);
    step("statusline", { conversation_id: "", agent_state: "initializing", email: "a@example.com" });
    assert.equal(JSON.parse(fs.readFileSync(path.join(dir, "antigravity-usage.json"), "utf8")).groups.length, 1);
    step("statusline", { conversation_id: "", agent_state: "idle", email: "a@example.com" });
    assert.equal(JSON.parse(fs.readFileSync(path.join(dir, "antigravity-usage.json"), "utf8")).groups.length, 1);
    step("statusline", { conversation_id: "", agent_state: "idle", email: "" });
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir, "antigravity-usage.json"), "utf8")).groups, []);
    step("statusline", {
      conversation_id: cid1,
      agent_state: "idle",
      email: "a@example.com",
      quota: { "gemini-5h": { remaining_fraction: 0.5, reset_time: "2026-11-28T08:28:49Z" } },
    });
    step("statusline", { conversation_id: cid1, agent_state: "working", email: "a@example.com" });
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir, "antigravity-usage.json"), "utf8")).groups, []);

    // 14. 跨进程 --resume 恢复旧死进程的同一 cid 时，清理死进程不得把活阅读映射覆写为 ended
    fs.writeFileSync(
      path.join(dir, "antigravity-pid-999999.json"),
      JSON.stringify({ session_id: "pid-999999", source: "antigravity", pid: 999999, conversation_id: cid1, state: "idle" })
    );
    handleEvent(
      "statusline",
      { conversation_id: cid1, agent_state: "idle", cwd: "/tmp/demo-project" },
      { dir, homeDir: home, processContext: { agyPid: 5353, tty: "/dev/ttys009" }, now: new Date((t += 1000)) }
    );
    assert.equal(fs.existsSync(path.join(dir, "antigravity-pid-999999.json")), false);
    assert.equal(fs.readFileSync(path.join(dir, ".reader", `antigravity-${cid1}`), "utf8").trim(), t1);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test("mergeAntigravitySettings 幂等合并、备份、保留自定义 statusLine 回退 title，并拒绝符号链接与非法 JSON", () => {
  const dir = makeTempDir("rg-agy-merge-");
  try {
    const settingsPath = path.join(dir, "settings.json");
    const hooksPath = path.join(dir, "hooks.json");
    fs.writeFileSync(settingsPath, JSON.stringify({ agentMode: "auto", colorScheme: "dark" }, null, 2) + "\n");
    fs.writeFileSync(hooksPath, JSON.stringify({ "user-linter": { PostToolUse: [] } }, null, 2) + "\n");

    const first = mergeAntigravitySettings({ settingsPath, hooksPath, stamp: "t1" });
    assert.equal(first.slot, "statusLine");
    assert.ok(first.settingsBackup && fs.existsSync(first.settingsBackup));
    assert.ok(first.hooksBackup && fs.existsSync(first.hooksBackup));
    const savedSettings = JSON.parse(fs.readFileSync(settingsPath, "utf8"));
    assert.equal(savedSettings.agentMode, "auto");
    assert.equal(savedSettings.statusLine.stack_with_default, true);
    const savedHooks = JSON.parse(fs.readFileSync(hooksPath, "utf8"));
    assert.ok(savedHooks["user-linter"]);
    assert.equal(savedHooks["red-green"].PreToolUse[0].matcher, "ask_question");
    assert.equal(savedHooks["red-green"].Stop.length, 1);

    // 幂等重跑不再改动也不再产生多余备份
    const second = mergeAntigravitySettings({ settingsPath, hooksPath, stamp: "t2" });
    assert.equal(second.settingsBackup, null);
    assert.equal(second.hooksBackup, null);

    // 已有自定义 statusLine 时不覆盖，自动改用 title
    fs.writeFileSync(settingsPath, JSON.stringify({ statusLine: { type: "command", command: "~/my-status.sh" } }, null, 2) + "\n");
    const fallbackTitle = mergeAntigravitySettings({ settingsPath, hooksPath, stamp: "t3" });
    assert.equal(fallbackTitle.slot, "title");
    const withTitle = JSON.parse(fs.readFileSync(settingsPath, "utf8"));
    assert.equal(withTitle.statusLine.command, "~/my-status.sh");
    assert.match(withTitle.title.command, /red-green-status\.sh statusline$/);

    // 两者均被占时不覆盖
    fs.writeFileSync(
      settingsPath,
      JSON.stringify({
        statusLine: { type: "command", command: "~/my-status.sh" },
        title: { type: "command", command: "~/my-title.sh" },
      }) + "\n"
    );
    const manual = mergeAntigravitySettings({ settingsPath, hooksPath, stamp: "t4" });
    assert.equal(manual.slot, "manual");
    assert.equal(manual.settingsBackup, null);

    // 拒绝符号链接（含悬空软链）与非法 JSON
    const linkPath = path.join(dir, "link.json");
    fs.symlinkSync(settingsPath, linkPath);
    assert.throws(() => mergeAntigravitySettings({ settingsPath: linkPath, hooksPath }), /符号链接/);
    const danglingLink = path.join(dir, "dangling.json");
    fs.symlinkSync(path.join(dir, "nonexistent.json"), danglingLink);
    assert.throws(() => mergeAntigravitySettings({ settingsPath: danglingLink, hooksPath }), /符号链接/);
    const brokenPath = path.join(dir, "broken.json");
    fs.writeFileSync(brokenPath, "{bad");
    assert.throws(() => mergeAntigravitySettings({ settingsPath: brokenPath, hooksPath }), /合法 JSON/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
