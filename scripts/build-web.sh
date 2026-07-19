#!/usr/bin/env bash
# Build the playable Aurora Run browser demo with Trunk.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v trunk >/dev/null 2>&1; then
  echo "Installing trunk (WASM web bundler)…"
  cargo install trunk --locked
fi

rustup target add wasm32-unknown-unknown >/dev/null
echo "Building Aurora Run → dist/aurora-run/"
cd examples/aurora_run
# Trunk expects an explicit boolean for NO_COLOR; Codex shells often export `1`.
env -u NO_COLOR trunk build --release
echo "Done. Serve with: cd examples/aurora_run && trunk serve"
