#!/usr/bin/env bash
# Enforce a deliberate shipping budget for the embedded Last Light web build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/games/last-light/dist"
MAX_WASM_BYTES=$((18 * 1024 * 1024))
MAX_SOURCE_ASSET_BYTES=$((12 * 1024 * 1024))

WASM="$(find "$DIST" -maxdepth 1 -name '*.wasm' -type f | head -n 1)"
if [[ -z "$WASM" ]]; then
  echo "No Last Light WASM artifact found. Run ./scripts/build-web.sh first." >&2
  exit 1
fi

wasm_bytes="$(stat -f '%z' "$WASM")"
asset_bytes="$(find "$ROOT/games/last-light/assets" -type f -name '*.png' -exec stat -f '%z' {} \; | awk '{sum += $1} END {print sum + 0}')"

echo "Last Light WASM: $wasm_bytes bytes (budget $MAX_WASM_BYTES)"
echo "Last Light source PNGs: $asset_bytes bytes (budget $MAX_SOURCE_ASSET_BYTES)"

[[ "$wasm_bytes" -le "$MAX_WASM_BYTES" ]]
[[ "$asset_bytes" -le "$MAX_SOURCE_ASSET_BYTES" ]]
