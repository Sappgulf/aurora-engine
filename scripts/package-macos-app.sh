#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/dist/Aurora Last Light.app"
BIN="$ROOT/target/release/last_light"
PLIST="$ROOT/tools/macos/LastLight-Info.plist"
COVER="$ROOT/games/last-light/assets/cover/aurora-last-light-cover-v001.png"

if [[ ! -x "$BIN" ]]; then
  echo "missing release binary: $BIN" >&2
  echo "run: cargo build -p last_light --release" >&2
  exit 1
fi

if [[ ! -f "$PLIST" ]]; then
  echo "missing bundle metadata: $PLIST" >&2
  exit 1
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/Last Light"
cp "$PLIST" "$APP/Contents/Info.plist"
if [[ -f "$COVER" ]]; then
  cp "$COVER" "$APP/Contents/Resources/cover.png"
fi
chmod 755 "$APP/Contents/MacOS/Last Light"

# Ad-hoc signing keeps the local bundle launchable without a paid developer
# identity. It is intentionally best-effort so packaging also works on CI.
if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true
fi

echo "Packaged: $APP"
