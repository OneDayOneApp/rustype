#!/usr/bin/env bash
# Install rustype as a per-user LaunchAgent so it starts at login.
set -euo pipefail

LABEL="com.rustype.agent"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Use the app bundle: macOS tracks menu-bar items by app identity, and the
# bundle gives us a stable one (com.rustype.desktop).
BIN="${RUSTYPE_BIN:-$ROOT/dist/Rustype.app/Contents/MacOS/rustype}"

if [[ "${1:-}" == "--remove" ]]; then
  launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
  rm -f "$PLIST"
  echo "removed $LABEL"
  exit 0
fi

if [[ ! -x "$BIN" ]]; then
  echo "binary not found: $BIN" >&2
  echo "build it first: cargo build --release" >&2
  exit 1
fi

mkdir -p "$HOME/Library/LaunchAgents"
cat > "$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$BIN</string><string>run</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/tmp/rustype.log</string>
  <key>StandardErrorPath</key><string>/tmp/rustype.log</string>
</dict>
</plist>
PLIST

launchctl bootout "gui/$(id -u)" "$PLIST" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
echo "installed $LABEL -> $BIN"
echo "logs: /tmp/rustype.log"
echo "note: grant Accessibility permission to $BIN"
