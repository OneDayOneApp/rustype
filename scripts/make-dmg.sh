#!/usr/bin/env bash
# Build a drag-to-Applications DMG from the packaged app bundle.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${1:-0.1.0}"
APP="$ROOT/dist/Rustype.app"

"$ROOT/scripts/make-app.sh"

STAGE="$(mktemp -d /tmp/rustype-dmg.XXXXXX)"
cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

cp -R "$APP" "$STAGE/Rustype.app"
ln -s /Applications "$STAGE/Applications"

cp "$ROOT/assets/icon.icns" "$STAGE/.VolumeIcon.icns" 2>/dev/null || true
SetFile -a C "$STAGE" 2>/dev/null || true

cat > "$STAGE/安装说明.txt" <<'TXT'
Rustype 安装说明

1. 将 Rustype.app 拖到右侧的 Applications 文件夹。
2. 首次打开若被系统拦截：右键 Rustype.app → 打开 → 仍要打开。
3. 打开「系统设置 → 隐私与安全性 → 辅助功能」，勾选 Rustype。
4. 启动后菜单栏会出现麦克风图标：
   - 点按右 Option：开始 / 停止听写
   - 右 Option + Space：询问任何问题
   - 右 Option + Shift：翻译

配置文件：~/Library/Application Support/Rustype/config.toml
TXT

DMG="$ROOT/dist/Rustype-$VERSION.dmg"
rm -f "$DMG"
hdiutil create \
  -volname "Rustype $VERSION" \
  -srcfolder "$STAGE" \
  -ov -format UDZO -fs HFS+ \
  "$DMG" >/dev/null

echo "built $DMG"
shasum -a 256 "$DMG"
