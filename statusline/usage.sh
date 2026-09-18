#!/bin/bash
# red-green: capture account usage for the desktop pets.
# Configured as Claude Code's statusLine command; receives the status line JSON
# on stdin and writes ~/.claude/session-status/usage.json. Prints nothing — the
# pets are the display. Runs on every status line redraw of every session, so it
# must stay fast and fail-silent.

exec 2>/dev/null

input=$(cat)
dir="$HOME/.claude/session-status"
file="$dir/usage.json"

# 只取两个限额窗口。rate_limits 缺席（API 计费、会话首次响应之前）时不动现有文件：
# 缺席不代表用量归零
usage=$(jq -c '.rate_limits
  | select(type == "object")
  | {five_hour, seven_day}
  | with_entries(select(.value | type == "object"))
  | with_entries(select(.value.used_percentage | type == "number"))
  | map_values({used_percentage, resets_at} | with_entries(select(.value != null)))
  | select(length > 0)' <<<"$input")
[ -z "$usage" ] && exit 0

# 值没变就不重写：宠物 app 监听整个目录，status line 每次重绘都会跑到这里
if [ -f "$file" ] && [ "$(jq -c 'del(.updated_at)' "$file")" = "$usage" ]; then
  exit 0
fi

# 先写临时文件再改名：app 不会读到写了一半的 JSON。
# 临时文件名随机：多个会话的 status line 可能同时重绘，固定名会互相截断
mkdir -p "$dir"
tmp=$(mktemp "$file.XXXXXX") || exit 0
if jq -c --arg now "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '. + {updated_at: $now}' <<<"$usage" > "$tmp"; then
  mv "$tmp" "$file"
else
  rm -f "$tmp"
fi
exit 0
