#!/bin/bash
# red-green: report Claude Code session state for the desktop pets.
# Invoked by Claude Code hooks; receives the hook JSON payload on stdin,
# writes ~/.claude/session-status/<session_id>.json (or removes it on SessionEnd).
# Must stay fast and fail-silent: it runs on every tool call of every session.

exec 2>/dev/null

input=$(cat)
dir="$HOME/.claude/session-status"
mkdir -p "$dir"

event=$(jq -r '.hook_event_name // empty' <<<"$input")
sid=$(jq -r '.session_id // empty' <<<"$input")
if [ -z "$event" ] || [ -z "$sid" ]; then exit 0; fi

file="$dir/$sid.json"

case "$event" in
  SessionEnd)
    rm -f "$file"
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) ${sid:0:8} SessionEnd -> (removed)" >> "$dir/.events.log"
    exit 0
    ;;
  SessionStart) state="idle" ;;
  UserPromptSubmit | PreToolUse | PostToolUse) state="running" ;;
  Notification) state="awaiting" ;;
  Stop)
    # 回合以提问收尾 → 轮到你；否则 → 已完成。
    # 只信最后一行：以问号收尾（容忍 markdown 收尾符），或明确的请求答复短语。
    # 更宽的匹配（如尾部 400 字符内任意问号、"是否"）在长篇分析文本里误报率太高。
    msg=$(jq -r '.last_assistant_message // empty' <<<"$input")
    last_line=$(printf '%s' "$msg" | grep -v '^[[:space:]]*$' | tail -1 | tail -c 240)
    if grep -qE '[？?]["*_)）】`[:space:]]*$' <<<"$last_line" ||
      grep -qE '请确认|请选择|请告诉我|要不要|你想|你希望|哪一?[个种项]' <<<"$last_line"; then
      state="your_turn"
    else
      state="completed"
    fi
    ;;
  StopFailure) state="aborted" ;;
  *) exit 0 ;;
esac

# running -> running rewrites carry no information; skip them so the
# file watcher in the pet app isn't spammed on every tool call.
if [ "$state" = "running" ] && [ -f "$file" ]; then
  prev=$(jq -r '.state // empty' "$file")
  if [ "$prev" = "running" ]; then exit 0; fi
fi

cwd=$(jq -r '.cwd // empty' <<<"$input")
if [ "$state" = "your_turn" ]; then
  # 轮到你时问题在消息尾部；留存尾部也方便日后排查启发式误判
  detail=$(jq -r '.message // .last_assistant_message // empty' <<<"$input" | tail -c 300)
else
  detail=$(jq -r '.message // .last_assistant_message // empty' <<<"$input" | head -c 300)
fi

# The controlling tty identifies which Terminal tab hosts this session
# (used for click-to-focus and acknowledgement detection). The hook itself
# has no tty, so walk up the process tree until one appears.
tty=""
pid=$$
for _ in 1 2 3 4 5 6 7 8; do
  t=$(ps -o tty= -p "$pid" | tr -d ' ')
  if [ -n "$t" ] && [ "$t" != "??" ]; then
    tty="/dev/$t"
    break
  fi
  pid=$(ps -o ppid= -p "$pid" | tr -d ' ')
  if [ -z "$pid" ] || [ "$pid" = "0" ] || [ "$pid" = "1" ]; then break; fi
done

# Keep the previously recorded tty if this invocation couldn't find one.
if [ -z "$tty" ] && [ -f "$file" ]; then
  tty=$(jq -r '.tty // empty' "$file")
fi

now=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# 名牌 = git 仓库根目录名：会话 cwd 会随 shell cd 漂移到子目录，
# 但项目身份应该稳定（app/src-tauri 里干活也还是 red-green）
project=""
if [ -n "$cwd" ]; then
  root=$(git -C "$cwd" rev-parse --show-toplevel 2>/dev/null)
  project=$(basename "${root:-$cwd}")
fi
[ -z "$project" ] && project="?"

jq -n \
  --arg sid "$sid" --arg state "$state" --arg cwd "$cwd" \
  --arg tty "$tty" --arg detail "$detail" --arg project "$project" \
  --arg since "$now" --arg event "$event" \
  '{session_id: $sid, state: $state, cwd: $cwd, project: $project,
    tty: $tty, detail: $detail, since: $since, event: $event}' > "$file"

# 滚动事件日志：诊断"宠物状态与体感不符"时看事件序列
log="$dir/.events.log"
echo "$now ${sid:0:8} $event -> $state" >> "$log"
if [ "$(wc -l < "$log")" -gt 2000 ]; then
  tail -n 1000 "$log" > "$log.tmp" && mv "$log.tmp" "$log"
fi

exit 0
