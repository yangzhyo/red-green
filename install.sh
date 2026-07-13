#!/bin/bash
# Install red-green's global hooks: link the status script into ~/.claude/hooks
# and merge the hook configuration into ~/.claude/settings.json.
set -euo pipefail

repo="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$HOME/.claude/hooks" "$HOME/.claude/session-status"
chmod +x "$repo/hooks/session-status.sh"
ln -sf "$repo/hooks/session-status.sh" "$HOME/.claude/hooks/red-green-status.sh"

node "$repo/scripts/merge-hooks.mjs"

echo "done. hooks take effect in newly started Claude Code sessions."
