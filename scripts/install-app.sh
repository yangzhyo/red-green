#!/bin/bash
# Build the release .app, install it to ~/Applications, and register a
# LaunchAgent so the pets start at login and revive after crashes.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"

cd "$repo/app"
pnpm tauri build

mkdir -p "$HOME/Applications"
rm -rf "$HOME/Applications/red-green.app"
cp -R "$repo/app/src-tauri/target/release/bundle/macos/red-green.app" "$HOME/Applications/"

plist="$HOME/Library/LaunchAgents/dev.y9g.red-green.plist"
mkdir -p "$HOME/Library/LaunchAgents"
cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.y9g.red-green</string>
  <key>ProgramArguments</key>
  <array><string>$HOME/Applications/red-green.app/Contents/MacOS/red-green</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
</dict>
</plist>
EOF

launchctl unload "$plist" 2>/dev/null || true
launchctl load "$plist"

echo "red-green.app installed to ~/Applications, launch-at-login registered."
