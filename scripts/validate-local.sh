#!/usr/bin/env bash
# Aurora's reproducible local release gate.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace --target wasm32-unknown-unknown
cargo clippy --workspace --target wasm32-unknown-unknown --all-targets -- -D warnings
cargo test -p last_light simulation::tests::reclaim_truth_trace_replays_through_victory_with_the_same_hash -- --exact

./scripts/build-web.sh
./scripts/build-platformer-web.sh
npm run test:browser

./scripts/check-web-budget.sh
./scripts/check-platformer-budget.sh

echo "Local Aurora validation passed."
