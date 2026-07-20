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
# Only tracked source art is part of the reproducible web artifact. This
# intentionally ignores local exploration copies (for example, a designer's
# "asset 2.png") without deleting or hiding them from the working tree.
asset_bytes=0
while IFS= read -r -d '' asset; do
  [[ "$asset" == *.png ]] || continue
  asset_bytes=$((asset_bytes + $(stat -f '%z' "$ROOT/$asset")))
done < <(git -C "$ROOT" ls-files -z -- 'games/last-light/assets')

echo "Last Light WASM: $wasm_bytes bytes (budget $MAX_WASM_BYTES)"
echo "Last Light source PNGs: $asset_bytes bytes (budget $MAX_SOURCE_ASSET_BYTES)"

[[ "$wasm_bytes" -le "$MAX_WASM_BYTES" ]]
[[ "$asset_bytes" -le "$MAX_SOURCE_ASSET_BYTES" ]]
