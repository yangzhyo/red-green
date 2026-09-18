#!/usr/bin/env node
// Merge red-green's Claude Code configuration into ~/.claude/settings.json:
// the session-state hooks and the usage status line.
// Idempotent: entries are recognized by the script path and replaced,
// never duplicated. A timestamped backup is written before any change.

import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const SETTINGS = path.join(os.homedir(), ".claude", "settings.json");
const HOOK_SCRIPT = path.join(os.homedir(), ".claude", "hooks", "red-green-status.sh");
const USAGE_SCRIPT = path.join(os.homedir(), ".claude", "red-green-usage.sh");

// Which events feed the pets, and which matcher (if any) filters them.
// Notification is filtered to genuine mid-turn blockage only. idle_prompt is
// deliberately excluded: it is a timer, not a semantic signal — see
// docs/protocol.md. auth_success / agent_completed etc. must not flip a
// session to awaiting either.
const EVENTS = {
  SessionStart: null,
  UserPromptSubmit: null,
  PreToolUse: null,
  PostToolUse: null,
  Notification: "permission_prompt|elicitation_dialog|agent_needs_input",
  Stop: null,
  StopFailure: null,
  SessionEnd: null,
};

const settings = JSON.parse(fs.readFileSync(SETTINGS, "utf8"));
const backup = `${SETTINGS}.bak.${new Date().toISOString().replace(/[:.]/g, "-")}`;
fs.copyFileSync(SETTINGS, backup);

settings.hooks ??= {};
for (const [event, matcher] of Object.entries(EVENTS)) {
  const entry = {
    ...(matcher ? { matcher } : {}),
    hooks: [{ type: "command", command: HOOK_SCRIPT, timeout: 10 }],
  };
  const existing = settings.hooks[event] ?? [];
  const others = existing.filter(
    (e) => !(e.hooks ?? []).some((h) => h.command === HOOK_SCRIPT)
  );
  settings.hooks[event] = [...others, entry];
}

// The status line is the only outlet for account usage (see docs/protocol.md),
// but it has a single slot: a status line the user already configured is left
// untouched, with a hint on how to chain ours in.
const current = settings.statusLine;
if (!current) {
  settings.statusLine = { type: "command", command: USAGE_SCRIPT };
  console.log("statusLine set to the red-green usage script");
} else if (current.command === USAGE_SCRIPT) {
  console.log("statusLine already points at the red-green usage script");
} else {
  console.log(
    `statusLine is already taken (${current.command ?? JSON.stringify(current)}); left as is.`
  );
  console.log(
    `to feed usage to the pets, have your status line script pipe its stdin JSON to ${USAGE_SCRIPT} as well (it prints nothing).`
  );
}

fs.writeFileSync(SETTINGS, JSON.stringify(settings, null, 2) + "\n");
console.log(`settings merged into ${SETTINGS}`);
console.log(`backup: ${backup}`);
