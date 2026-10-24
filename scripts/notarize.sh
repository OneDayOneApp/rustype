#!/usr/bin/env bash
# Sign a DMG with Developer ID, notarize it and staple the ticket.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG="${1:-$ROOT/dist/Rustype-0.1.0-universal.dmg}"
SIGN_ID="${SIGN_ID:?set SIGN_ID to \"Developer ID Application: Name (TEAMID)\"}"
PROFILE="${NOTARY_PROFILE:-rustype}"

echo "==> signing $DMG"
codesign --force --timestamp --sign "$SIGN_ID" "$DMG"

echo "==> notarizing (may take a few minutes)"
xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait

echo "==> stapling"
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"

echo "==> gatekeeper check"
spctl -a -vvv -t install "$DMG" || true

echo "done: $DMG"
