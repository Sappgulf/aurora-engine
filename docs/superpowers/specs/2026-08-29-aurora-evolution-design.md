# Aurora Engine Evolution Design

**Date:** 2026-08-29

**Goal:** Execute four sequential, independently verifiable improvements that make Aurora safer to drive, more predictable across native/WASM, more diagnosable for content tooling, and more scalable for tactical simulation.

**Scope:** Track A (foundation hardening), Track B (renderer/resource budgets), Track C (authoring and runtime contracts), and Track D (simulation scale). Each track is intentionally bounded to a shippable slice; the larger idea backlog remains available for follow-up iterations.

## Constraints

- Preserve the existing public gameplay API wherever a compatible additive change is possible.
- Keep native and `wasm32-unknown-unknown` builds supported.
- Add regression tests before production behavior changes.
- Do not add a dependency unless the existing workspace cannot provide the capability.
- Preserve all pre-existing user changes in the dirty worktree.
- Run the relevant unit, integration, format, clippy, WASM, and browser checks before advancing tracks.
- Keep agent tooling development-only, loopback-bound, bounded, and unable to execute arbitrary shell commands.

## Track A — Foundation hardening

### Input contract

The app shell will mark the fixed-step phase so edge-triggered keyboard, mouse, and gamepad reads are visible to the first fixed step only, while remaining available to the variable update callback. This preserves existing `Input` query methods and removes per-game catch-up guards. Synthetic gamepad button updates will accumulate a press edge for the whole rendered frame, matching keyboard and mouse behavior.

### WASM quality

Remove the duplicate module-level WASM cfg, isolate native-only save tests, and make renderer declarations that do not exist on WASM explicitly conditional or dead-code-clean. Strict WASM clippy becomes a required local validation lane.

### Agent limits

Bound protocol frame size, partial-buffer growth, requests per poll, action/path text, and outbound Python frames. Oversized native clients are disconnected without affecting the game loop; malformed bounded frames receive normal protocol errors.

### Validation truth

Add a reproducible platformer Trunk build wrapper, include strict WASM clippy in local validation, and make the browser lane runnable from the same validation entry point after both web artifacts are rebuilt. Update the report only from fresh command results.

## Track B — Renderer and resource budgets

Add explicit per-frame budgets for normal sprites, debug sprites, and lights. Dropped work is counted in `RenderStats`/`DiagnosticSnapshot` rather than silently disappearing. Add preflight texture limits for decoded dimensions and byte payloads, with tests that exercise the pure validation path. Keep the current stable texture-handle model and batching order intact.

The first slice is a safety/performance contract, not a render-graph rewrite: it prevents runaway queues and pathological uploads while leaving a future render graph free to replace the internals.

## Track C — Authoring and runtime contracts

Connect the existing `AssetManifest`/`AssetLoadQueue` to the app lifecycle through an additive `Game::asset_manifest` hook. Games that expose a manifest get meaningful diagnostics; the synchronous existing loaders mark their manifest entries ready after `on_start`, preserving current startup behavior while making the contract observable. Last Light will expose its authoritative manifest.

Extend asset diagnostics with total and ready counts. Add semantic Last Light agent state only where it can be produced from existing simulation truth, keeping pixel checks as a visual supplement rather than the sole acceptance contract.

## Track D — Simulation scale

Reuse the existing deterministic unit spatial index during RTS updates. Build a start-of-tick snapshot, query candidate unit IDs by cell, restore stable unit order before applying separation, and use an ID map for attack/follow target lookup. Preserve current motion math and exact state semantics while eliminating the all-units scan for common local interactions. Add finite-position guards and candidate-query regression tests.

Obstacle broadphase, flow fields, and nav-query scheduling remain follow-up work after profiling confirms the unit broadphase is the dominant cost.

## Verification gates

After each track:

1. Run focused regression tests and the relevant package tests.
2. Run `cargo fmt --all -- --check` and native clippy with warnings denied.
3. Run WASM check/clippy for engine-facing changes.
4. Run browser tests after web artifacts are rebuilt when runtime behavior changes.
5. Inspect `git diff --check` and the changed-file list before proceeding.

## Non-goals for this pass

- A full ECS migration.
- A complete render graph or shader/material rewrite.
- An in-engine editor UI.
- Replacing all Last Light campaign code in one change.
- Background worker threads or nondeterministic simulation jobs.
