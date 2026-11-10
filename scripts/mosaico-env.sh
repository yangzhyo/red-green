#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
sdk=${RED_GREEN_IDF_PATH:-${IDF_PATH:-}}
if [[ -z "$sdk" || ! -f "$sdk/export.sh" ]]; then
  echo '设置 RED_GREEN_IDF_PATH 或 IDF_PATH，SDK 锁定提交见 docs/mosaico.md。' >&2
  exit 1
fi
if [[ "$(git -C "$sdk" rev-parse HEAD)" != 0e6b480f858c69862ffb6001d96e9eef74599a6e ]]; then
  echo 'ESP-IDF 提交与已验证版本不符，见 docs/mosaico.md。' >&2
  exit 1
fi
if [[ "$(git -C "$repo/firmware/vendor/esp-mosaico-bsp" rev-parse HEAD)" != 716fc7bca30ed9c61166a0343bf6f5f995b5131f ]]; then
  echo '运行 git submodule update --init --recursive 恢复锁定 BSP。' >&2
  exit 1
fi
source "$sdk/export.sh" >/dev/null
cd "$repo/firmware/mosaico"
exec idf.py "$@"
