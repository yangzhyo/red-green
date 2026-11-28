#!/usr/bin/env node
// 将 red-green 的 Antigravity CLI 配置幂等合并进：
// 1. ~/.gemini/antigravity-cli/settings.json（statusLine 优先，stack_with_default: true；被占时回退 title）
// 2. ~/.gemini/config/hooks.json（仅登记 red-green 的 Stop 与 ask_question 钩子）
// 修改前生成带时间戳的备份；拒绝修改符号链接或非法 JSON。

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_SETTINGS = path.join(os.homedir(), ".gemini", "antigravity-cli", "settings.json");
const DEFAULT_HOOKS = path.join(os.homedir(), ".gemini", "config", "hooks.json");
const DEFAULT_SCRIPT = "~/.gemini/antigravity-cli/red-green-status.sh";

function isOurs(command, scriptPath) {
  if (typeof command !== "string") return false;
  const trimmed = command.trim();
  const expanded = scriptPath.startsWith("~/")
    ? path.join(os.homedir(), scriptPath.slice(2))
    : scriptPath;
  return (
    trimmed.startsWith(`${scriptPath} `) ||
    trimmed === scriptPath ||
    trimmed.startsWith(`${expanded} `) ||
    trimmed === expanded
  );
}

function readJsonObject(filePath, label) {
  if (fs.existsSync(filePath)) {
    const lst = fs.lstatSync(filePath);
    if (lst.isSymbolicLink()) {
      throw new Error(`${label}（${filePath}）是符号链接，请手动配置后再重试。`);
    }
    const raw = fs.readFileSync(filePath, "utf8");
    let parsed;
    try {
      parsed = JSON.parse(raw);
    } catch (e) {
      throw new Error(`${label}（${filePath}）不是合法 JSON：${e.message}`);
    }
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new Error(`${label}（${filePath}）顶层必须是 JSON 对象。`);
    }
    return { exists: true, raw, data: parsed, mode: lst.mode & 0o777 };
  }
  return { exists: false, raw: null, data: {}, mode: 0o600 };
}

function writeJsonWithBackup(filePath, data, info, stamp) {
  const output = `${JSON.stringify(data, null, 2)}\n`;
  if (info.exists && info.raw === output) {
    return null;
  }
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  let backup = null;
  if (info.exists) {
    backup = `${filePath}.red-green-${stamp}.bak`;
    fs.copyFileSync(filePath, backup);
  }
  const tmp = `${filePath}.red-green-${stamp}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, output, { mode: info.mode || 0o600 });
  fs.renameSync(tmp, filePath);
  return backup;
}

export function mergeAntigravitySettings({
  settingsPath = DEFAULT_SETTINGS,
  hooksPath = DEFAULT_HOOKS,
  scriptPath = DEFAULT_SCRIPT,
  stamp = new Date().toISOString().replace(/[:.]/g, "-"),
} = {}) {
  const settingsInfo = readJsonObject(settingsPath, "Antigravity CLI settings.json");
  const hooksInfo = readJsonObject(hooksPath, "Antigravity hooks.json");
  const settings = structuredClone(settingsInfo.data);
  const hooks = structuredClone(hooksInfo.data);

  const statusCmd = `${scriptPath} statusline`;
  let slot = "none";
  const curStatus = settings.statusLine;
  const curTitle = settings.title;
  const statusFree =
    curStatus == null ||
    typeof curStatus === "boolean" ||
    (typeof curStatus === "object" && (!curStatus.command || isOurs(curStatus.command, scriptPath)));
  const titleFree =
    curTitle == null ||
    typeof curTitle === "boolean" ||
    (typeof curTitle === "object" && (!curTitle.command || isOurs(curTitle.command, scriptPath)));

  if (statusFree) {
    settings.statusLine = {
      type: "command",
      command: statusCmd,
      stack_with_default: true,
    };
    slot = "statusLine";
  } else if (titleFree) {
    settings.title = {
      type: "command",
      command: statusCmd,
    };
    slot = "title";
  } else {
    slot = "manual";
  }

  hooks["red-green"] = {
    PreToolUse: [
      {
        matcher: "ask_question",
        hooks: [
          {
            type: "command",
            command: `${scriptPath} hook pretool`,
            timeout: 5,
          },
        ],
      },
    ],
    PostToolUse: [
      {
        matcher: "ask_question",
        hooks: [
          {
            type: "command",
            command: `${scriptPath} hook posttool`,
            timeout: 5,
          },
        ],
      },
    ],
    Stop: [
      {
        type: "command",
        command: `${scriptPath} hook stop`,
        timeout: 5,
      },
    ],
  };

  const settingsBackup =
    slot === "manual" ? null : writeJsonWithBackup(settingsPath, settings, settingsInfo, stamp);
  const hooksBackup = writeJsonWithBackup(hooksPath, hooks, hooksInfo, stamp);
  return { slot, settingsBackup, hooksBackup, settingsPath, hooksPath };
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
  try {
    const result = mergeAntigravitySettings();
    if (result.slot === "statusLine") {
      console.log(`已配置 statusLine（保留内置底栏）：${result.settingsPath}`);
    } else if (result.slot === "title") {
      console.log(`检测到已有自定义 statusLine，未覆盖；已改用 title 槽位：${result.settingsPath}`);
    } else {
      console.log("检测到 statusLine 与 title 均已被自定义脚本占用，未覆盖你的 settings.json。");
      console.log(`请将自定义 statusLine 脚本收到的 stdin JSON 同步转发给：${DEFAULT_SCRIPT} statusline`);
    }
    if (result.settingsBackup) console.log(`settings.json 备份：${result.settingsBackup}`);
    console.log(`已合并 Antigravity CLI 钩子（仅 Stop 与 ask_question，按 antigravity-cli 过滤）：${result.hooksPath}`);
    if (result.hooksBackup) console.log(`hooks.json 备份：${result.hooksBackup}`);
  } catch (e) {
    console.error(`Antigravity CLI 接入配置失败：${e.message}`);
    process.exit(1);
  }
}
