#!/bin/bash
# Regenerate the README art from the real pet sprites (app/ui/sprites.js) so the
# images never drift from the actual art. Two headless-Chrome screenshots, each
# straight to PNG at 2x; every stage's window size equals its artwork size, so
# the shot needs no cropping. Rerun after changing sprites.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
art="$repo/scripts/readme-art"
out="$repo/docs/assets"
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

mkdir -p "$out"

shot() { # <html-abs-path> <width> <height> <png-abs-path>
  "$chrome" --headless=new --disable-gpu --hide-scrollbars \
    --force-device-scale-factor=2 --window-size="$2,$3" \
    --virtual-time-budget=2500 --screenshot="$4" "file://$1" 2>/dev/null
}

shot "$art/hero.html"   1200 440 "$out/hero.png"
shot "$art/legend.html" 1380 508 "$out/states.png"

echo "README art regenerated → docs/assets/hero.png, docs/assets/states.png"
