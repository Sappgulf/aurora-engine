# Authoring Contracts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect authoritative game asset manifests to runtime diagnostics and expose meaningful loading state without changing current synchronous loaders.

**Architecture:** `Game::asset_manifest` is an additive opt-in hook. The app constructs `AssetLoadQueue` before entering the event loop, then marks manifest entries ready immediately after the game’s existing `on_start` completes. Diagnostics report total, ready, progress, and failures; games without manifests retain the current empty-queue behavior.

**Tech Stack:** Rust, serde_json, existing `AssetManifest` and `AssetLoadQueue`.

**Spec:** `docs/superpowers/specs/2026-08-29-aurora-evolution-design.md`

## Global Constraints

- Do not move Last Light’s current texture loading in this slice.
- Do not fabricate readiness before `Game::on_start` returns.
- The hook must default to `None` so every existing game compiles unchanged.
- Keep manifest ordering deterministic.

---

### Task 1: Add manifest completion semantics

**Files:**
- Modify: `crates/aurora-engine/src/loader.rs:30-115`
- Test: `crates/aurora-engine/src/loader.rs:120-160`

**Interfaces:**
- Produces `AssetLoadQueue::mark_all_ready(&mut self)`.
- `mark_all_ready` clears pending work and returns the number of entries transitioned to `Ready`.

- [ ] **Step 1: Write the failing test**

Extend the queue test to call `mark_all_ready` on a fresh manifest queue, assert the returned count equals `total()`, `ready_count()` equals `total()`, and `is_complete()` is true.

- [ ] **Step 2: Run the test to verify failure**

Run: `cargo test -p aurora-engine loader::tests::queue_reports_progress_for_success_and_failure -- --exact`

Expected: compilation failure because `mark_all_ready` does not exist.

- [ ] **Step 3: Implement completion**

Iterate entries in the deterministic `BTreeMap`, transition queued/loading entries to `Ready`, clear errors, clear `pending`, and return the transition count. Leave already ready/failed entries unchanged.

- [ ] **Step 4: Run loader tests**

Run: `cargo test -p aurora-engine loader::tests`

Expected: all loader tests pass.

### Task 2: Add app manifest hook and richer asset diagnostics

**Files:**
- Modify: `crates/aurora-engine/src/app.rs:1-80,115-145,560-590`
- Modify: `crates/aurora-engine/src/diagnostics.rs:10-45`
- Modify: `crates/aurora-engine/src/lib.rs:40-60,80-95`
- Test: `crates/aurora-engine/src/diagnostics.rs:100-130`

**Interfaces:**
- `Game::asset_manifest(&self) -> Option<AssetManifest>` defaults to `None`.
- `DiagnosticSnapshot` gains `total_assets` and `ready_assets`.

- [ ] **Step 1: Add failing diagnostic assertions**

Create a two-entry manifest queue, mark both ready, capture diagnostics, and assert total/ready counts are two.

- [ ] **Step 2: Run the test to verify failure**

Run: `cargo test -p aurora-engine diagnostics::tests -- --nocapture`

Expected: compilation failure because the fields and hook are missing.

- [ ] **Step 3: Implement the hook and lifecycle**

Import `AssetManifest`, add the default trait method, construct the queue from `game.asset_manifest()` in `run_result`, and call `mark_all_ready()` after `game.on_start(&mut renderer)` returns. Add total/ready values to snapshots and re-export no new type because `AssetManifest` is already public.

- [ ] **Step 4: Run engine tests and native clippy**

Run: `cargo test -p aurora-engine && cargo clippy -p aurora-engine --all-targets --all-features -- -D warnings`

Expected: exit 0.

### Task 3: Expose Last Light’s authoritative manifest

**Files:**
- Modify: `games/last-light/src/main.rs:5658-5680`
- Test: `games/last-light/src/assets.rs:760-880`

**Interfaces:**
- `LastLight` implements `Game::asset_manifest` by returning `Some(assets::manifest())`.

- [ ] **Step 1: Add a manifest-size regression assertion**

Extend the existing Last Light asset test to assert the game-facing manifest contains the same number of entries as `TextureAsset::ALL`.

- [ ] **Step 2: Run the focused test to verify the integration is absent**

Run: `cargo test -p last_light assets::tests -- --nocapture`

Expected: the asset-level test passes, but the trait implementation is not yet present; use compilation/trait inspection as the red signal for the new hook test.

- [ ] **Step 3: Implement the additive hook**

Add `fn asset_manifest(&self) -> Option<AssetManifest> { Some(assets::manifest()) }` to the Last Light `Game` implementation and import the already re-exported `AssetManifest` only if the compiler requires the explicit type.

- [ ] **Step 4: Verify Last Light diagnostics contract**

Run: `cargo test -p last_light && cargo check -p last_light --all-features`

Expected: exit 0.

