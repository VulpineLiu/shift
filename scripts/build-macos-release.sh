#!/bin/bash
# Build the universal macOS DMG and copy it to the versioned release name.
#
#   scripts/build-macos-release.sh [--skip-build]
#
# Produces artifacts/Shift-<version>-macOS-universal.dmg.
set -euo pipefail
cd "$(dirname "$0")/.."

# The Homebrew `cargo` on this host uses the non-rustup "system" toolchain, which
# cannot build the second slice of a universal binary.
export PATH="$HOME/.cargo/bin:$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
export RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin

SKIP_BUILD=0
usage() { echo "usage: $0 [--skip-build]" >&2 && exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1 ;;
    *) usage ;;
  esac
  shift
done

VERSION=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle"
APP="$BUNDLE_DIR/macos/Shift.app"
OUTPUT="artifacts/Shift-$VERSION-macOS-universal.dmg"

if [ "$SKIP_BUILD" != "1" ]; then
  pnpm mac:drag-bridge
  pnpm tauri build --target universal-apple-darwin --bundles app,dmg
fi

test -d "$APP"
DMG=$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -type f -name '*.dmg' -print -quit)
test -n "$DMG"

codesign --verify --deep --strict "$APP"
hdiutil verify "$DMG" >/dev/null
mkdir -p artifacts
cp -f "$DMG" "$OUTPUT"
hdiutil verify "$OUTPUT" >/dev/null

echo "== macOS release =="
ls -lh "$OUTPUT"
