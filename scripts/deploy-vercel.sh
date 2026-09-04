#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "Ensuring browser deliverables are current..."
./scripts/build-web.sh
./scripts/build-platformer-web.sh

if [[ ! -d "${ROOT}/games/last-light/dist" ]]; then
  echo "Missing games/last-light/dist. Build step failed." >&2
  exit 1
fi

if [[ ! -d "${ROOT}/demos/platformer/dist" ]]; then
  echo "Missing demos/platformer/dist. Build step failed." >&2
  exit 1
fi

echo "Deploying to Vercel (production)..."
npx vercel --prod --yes
