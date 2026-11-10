#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
out=${1:-"$repo/firmware/mosaico/main/generated"}
cache=$(mktemp -d)
trap 'rm -rf "$cache"' EXIT
base=https://raw.githubusercontent.com/notofonts/noto-cjk/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans
curl --fail --location --retry 3 "$base/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf" -o "$cache/font.otf"
actual=$(shasum -a 256 "$cache/font.otf" | cut -d ' ' -f 1)
[[ "$actual" == 2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b ]] || { echo '字体 SHA256 不符' >&2; exit 1; }
mkdir -p "$out"
curl --fail --location --retry 3 "$base/LICENSE" -o "$out/FONT-LICENSE.txt"
for size in 20 28; do
  "$repo/app/node_modules/.bin/lv_font_conv" --font "$cache/font.otf" -r 0x20-0xFFFF \
    --size "$size" --bpp 1 --no-kerning --format bin -o "$out/font$size.bin"
done
node "$repo/scripts/gen-mosaico-assets.mjs" "$out"
