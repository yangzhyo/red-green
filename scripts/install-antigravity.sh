#!/bin/bash
# 一次性配置 Antigravity CLI（statusLine 与极简 hooks.json）；日常仍直接运行 agy。
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
command -v agy >/dev/null || { echo '请先安装 Antigravity CLI（agy）。' >&2; exit 1; }
command -v node >/dev/null || { echo '请先安装 Node.js。' >&2; exit 1; }

agy --version
mkdir -p "$HOME/.gemini/antigravity-cli" "$HOME/.gemini/config" "$HOME/.claude/session-status"
chmod +x "$repo/hooks/antigravity.sh" "$repo/hooks/antigravity-state.mjs"
ln -sf "$repo/hooks/antigravity.sh" "$HOME/.gemini/antigravity-cli/red-green-status.sh"

node "$repo/scripts/merge-antigravity-settings.mjs"

echo "配置完成。新启动的 Antigravity CLI 会话会自动接入 red-green。"
