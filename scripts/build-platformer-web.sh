#!/usr/bin/env bash
# Build the Aurora platformer browser demo with Trunk.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v trunk >/dev/null 2>&1; then
  echo "Installing trunk (WASM web bundler)…"
  cargo install trunk --locked
fi

rustup target add wasm32-unknown-unknown >/dev/null
echo "Building Platformer → dist/"
cd demos/platformer
# Trunk treats NO_COLOR as a boolean; some Codex shells export the value 1.
env -u NO_COLOR trunk build --release
echo "Done. Serve with: cd demos/platformer && trunk serve"
