#!/bin/bash
# red-green: Antigravity CLI statusLine / hooks 统一入口。
# 必须始终快速且静默返回：statusLine 零输出以保留默认底栏，hook 按契约输出 allow / {}。

exec 2>/dev/null
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"

mode="${1:-statusline}"
sub="${2:-}"

fallback() {
  if [ "$mode" = "hook" ]; then
    if [ "$sub" = "posttool" ]; then
      printf '{}\n'
    else
      printf '{"decision":"allow"}\n'
    fi
  fi
  exit 0
}

src="$0"
while [ -L "$src" ]; do
  dir=$(cd -P "$(dirname "$src")" && pwd)
  target=$(readlink "$src")
  case "$target" in
    /*) src="$target" ;;
    *) src="$dir/$target" ;;
  esac
done
script_dir=$(cd -P "$(dirname "$src")" && pwd)
entry="$script_dir/antigravity-state.mjs"

command -v node >/dev/null && [ -f "$entry" ] || fallback
node "$entry" "$mode" "$sub" || fallback
exit 0
