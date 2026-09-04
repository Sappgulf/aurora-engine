#!/usr/bin/env bash
# Enforce a deliberate shipping budget for the platformer web build.
#
# The platformer is the engine's lightweight showcase: everything is
# procedural (no PNG assets), so the budget guards against accidental
# dependency bloat rather than art size.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/demos/platformer/dist"
MAX_WASM_BYTES=$((3 * 1024 * 1024))

WASM="$(find "$DIST" -maxdepth 1 -name '*_bg.wasm' -type f | head -n 1)"
if [[ -z "$WASM" ]]; then
  echo "No platformer WASM artifact found. Run 'trunk build --release' in demos/platformer first." >&2
  exit 1
fi

wasm_bytes="$(stat -f '%z' "$WASM")"
echo "Platformer WASM: $wasm_bytes bytes (budget $MAX_WASM_BYTES)"

if [[ "$wasm_bytes" -gt "$MAX_WASM_BYTES" ]]; then
  echo "Platformer WASM exceeds its 3 MiB budget — audit new dependencies before shipping." >&2
  exit 1
fi
