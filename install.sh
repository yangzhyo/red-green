#!/bin/bash
# Install red-green's Claude Code integration: link the hook and status line
# scripts into ~/.claude and merge their configuration into ~/.claude/settings.json.
set -euo pipefail

repo="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$HOME/.claude/hooks" "$HOME/.claude/session-status"
chmod +x "$repo/hooks/session-status.sh" "$repo/statusline/usage.sh"
ln -sf "$repo/hooks/session-status.sh" "$HOME/.claude/hooks/red-green-status.sh"
ln -sf "$repo/statusline/usage.sh" "$HOME/.claude/red-green-usage.sh"

node "$repo/scripts/merge-settings.mjs"

echo "done. hooks and status line take effect in newly started Claude Code sessions."
