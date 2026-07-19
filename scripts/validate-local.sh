#!/usr/bin/env bash
# Aurora's reproducible local release gate. This replaces hosted CI by design.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace --target wasm32-unknown-unknown
cargo test -p last_light simulation::tests::reclaim_truth_trace_replays_through_victory_with_the_same_hash -- --exact

if [[ -d games/last-light/dist ]]; then
  ./scripts/check-web-budget.sh
fi

echo "Local Aurora validation passed."
