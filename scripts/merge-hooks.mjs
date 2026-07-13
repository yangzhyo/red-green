#!/usr/bin/env node
// Merge red-green's hook configuration into ~/.claude/settings.json.
// Idempotent: entries are recognized by the hook script path and replaced,
// never duplicated. A timestamped backup is written before any change.

import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const SETTINGS = path.join(os.homedir(), ".claude", "settings.json");
const SCRIPT = path.join(os.homedir(), ".claude", "hooks", "red-green-status.sh");

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
    hooks: [{ type: "command", command: SCRIPT, timeout: 10 }],
  };
  const existing = settings.hooks[event] ?? [];
  const others = existing.filter(
    (e) => !(e.hooks ?? []).some((h) => h.command === SCRIPT)
  );
  settings.hooks[event] = [...others, entry];
}

fs.writeFileSync(SETTINGS, JSON.stringify(settings, null, 2) + "\n");
console.log(`hooks merged into ${SETTINGS}`);
console.log(`backup: ${backup}`);
