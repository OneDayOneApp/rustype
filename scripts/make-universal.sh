#!/usr/bin/env bash
# Build arm64 + x86_64 slices, combine them into a Universal binary and
# produce a Universal .app + DMG.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-0.1.0-universal}"

rustup target add x86_64-apple-darwin --toolchain stable >/dev/null 2>&1 || true

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  echo "==> building $target"
  rustup run stable cargo build --release --target "$target"
done

mkdir -p target/universal
lipo -create \
  target/aarch64-apple-darwin/release/rustype \
  target/x86_64-apple-darwin/release/rustype \
  -output target/universal/rustype
lipo -info target/universal/rustype

export RUSTYPE_BIN="$ROOT/target/universal/rustype"
"$ROOT/scripts/make-dmg.sh" "$VERSION"
