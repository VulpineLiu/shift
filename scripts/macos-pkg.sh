#!/bin/bash
# Wrap a built Shift.app in a .pkg installer for the release channel.
#
#   scripts/macos-pkg.sh <path/to/Shift.app> <output.pkg> [version]
#
# Why a package at all: this build is signed with a self-signed identity and is not
# notarized, so Gatekeeper calls it an unidentified developer. Dragging the app out of
# a disk image hands that verdict to the app itself, and a fresh machine refuses to open
# it until the user allows it in System Settings > Privacy & Security. `installer` does
# not propagate the quarantine flag onto what it installs, so a .pkg-installed copy
# launches with no prompt at all. The disk image stays the auto-update transport, which
# needs no administrator password; the package is what a new machine should install.
#
# The payload keeps the signature the app was built with; pkgbuild does not re-sign or
# strip it. An installer-signing identity would let the package itself be signed too,
# which needs a certificate with the Installer Signing purpose -- productsign rejects
# an application identity, and this repository's build has only that one.
set -euo pipefail

APP="${1:?usage: $0 <path/to/Shift.app> <output.pkg> [version]}"
OUT="${2:?usage: $0 <path/to/Shift.app> <output.pkg> [version]}"
VERSION="${3:-$(plutil -extract CFBundleShortVersionString raw "$APP/Contents/Info.plist" 2>/dev/null || echo 0.0.0)}"

[ -d "$APP" ] || { echo "not an app bundle: $APP" >&2; exit 2; }

STAGE=$(mktemp -d /tmp/shift-pkg-stage.XXXXXX)
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/Applications"
# ditto, not cp: it keeps resource forks, symlinks and code signatures intact.
ditto "$APP" "$STAGE/Applications/$(basename "$APP")"

# install-location is the folder the payload is rooted at, so the app lands in
# /Applications for every user of the machine.
pkgbuild \
  --version "$VERSION" \
  --identifier "$(plutil -extract CFBundleIdentifier raw "$APP/Contents/Info.plist" 2>/dev/null || echo app.nex.shift)" \
  --install-location /Applications \
  --root "$STAGE/Applications" \
  --ownership preserve \
  "$OUT"

echo "wrote $OUT ($(stat -f%z "$OUT") bytes, version $VERSION)"
echo "payload check:"
pkgutil --payload-files "$OUT" | head -4
