#!/bin/bash
# 一次性配置终端标题；日常仍直接运行 codex。
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
command -v codex >/dev/null || { echo '请先安装 Codex CLI。' >&2; exit 1; }
codex --version
cargo run --quiet --manifest-path "$repo/app/src-tauri/Cargo.toml" --bin red-green -- --install-codex
