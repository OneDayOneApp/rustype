#!/usr/bin/env bash
# Build a minimal Rustype.app bundle around target/release/rustype.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/dist/Rustype.app"
mkdir -p "$APP/Contents/MacOS"
if [[ ! -f "$APP/Contents/Info.plist" ]]; then
  echo "missing $APP/Contents/Info.plist" >&2
  exit 1
fi
BIN="${RUSTYPE_BIN:-$ROOT/target/release/rustype}"
if [[ ! -f "$BIN" ]]; then
  echo "missing binary $BIN" >&2
  exit 1
fi
cp "$BIN" "$APP/Contents/MacOS/rustype"
mkdir -p "$APP/Contents/Resources"
if [[ -f "$ROOT/assets/icon.icns" ]]; then
  cp "$ROOT/assets/icon.icns" "$APP/Contents/Resources/icon.icns"
fi
if ! /usr/libexec/PlistBuddy -c "Print :CFBundleIconFile" "$APP/Contents/Info.plist" >/dev/null 2>&1; then
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string icon.icns" "$APP/Contents/Info.plist"
fi
# Replacing the executable invalidates the bundle signature. With SIGN_ID set
# (Developer ID) sign for distribution; otherwise ad-hoc sign for local use.
if [[ -n "${SIGN_ID:-}" ]]; then
  codesign --force --options runtime --timestamp \
    --identifier com.rustype.desktop \
    --sign "$SIGN_ID" "$APP"
else
  codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true
fi
echo "built $APP"
