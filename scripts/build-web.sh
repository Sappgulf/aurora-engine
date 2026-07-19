#!/usr/bin/env bash
# Build the triangle demo for the browser with Trunk.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v trunk >/dev/null 2>&1; then
  echo "Installing trunk (WASM web bundler)…"
  cargo install trunk --locked
fi

rustup target add wasm32-unknown-unknown >/dev/null
echo "Building web playground → dist/"
cd examples/playground
trunk build --release
echo "Done. Serve with:  cd examples/playground && trunk serve"
echo "Or: python3 -m http.server -d ../../dist 8080"
