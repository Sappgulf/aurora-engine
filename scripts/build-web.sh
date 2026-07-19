#!/usr/bin/env bash
# Build the flagship Last Light browser game with Trunk.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v trunk >/dev/null 2>&1; then
  echo "Installing trunk (WASM web bundler)…"
  cargo install trunk --locked
fi

rustup target add wasm32-unknown-unknown >/dev/null
echo "Building Last Light → dist/last-light/"
cd games/last-light
# Trunk expects an explicit boolean for NO_COLOR; Codex shells often export `1`.
env -u NO_COLOR trunk build --release
echo "Done. Serve with: cd games/last-light && trunk serve"
