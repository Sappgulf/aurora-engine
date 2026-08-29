# Renderer Budgets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bound per-frame render work and texture uploads while making dropped work visible to diagnostics.

**Architecture:** `RenderBudget` is a portable value object owned by `Renderer`. Queueing APIs enforce separate sprite, debug-sprite, and light limits; `RenderStats` reports dropped counts. Texture preflight validates dimensions and decoded bytes before GPU creation while preserving the existing `Texture::from_rgba` call shape.

**Tech Stack:** Rust, wgpu 24, image, bytemuck.

**Spec:** `docs/superpowers/specs/2026-08-29-aurora-evolution-design.md`

## Global Constraints

- Keep sprite ordering, texture handles, and existing render output unchanged below the configured limits.
- Defaults must be high enough to preserve current games.
- No new dependency.
- Tests for queue and texture limits must not require a live GPU adapter.

---

### Task 1: Add pure render-budget behavior tests

**Files:**
- Modify: `crates/aurora-engine/src/renderer.rs:25-90,1515-1605,2585-2730`
- Test: `crates/aurora-engine/src/renderer.rs:2590-2640`

**Interfaces:**
- Produces `pub struct RenderBudget { pub max_sprites: usize, pub max_debug_sprites: usize, pub max_lights: usize }`.
- Produces `RenderStats::dropped_sprites`, `dropped_debug_sprites`, and `dropped_lights`.

- [ ] **Step 1: Write failing pure queue tests**

Test a helper that accepts items until a limit and counts rejected items. Test that zero-valued public budgets normalize to one. Test texture preflight acceptance/rejection for a valid 2×2 payload, an oversized dimension, and an oversized byte count.

- [ ] **Step 2: Run focused tests to verify failure**

Run: `cargo test -p aurora-engine renderer::tests texture::tests -- --nocapture`

Expected: the new types/helpers are missing.

- [ ] **Step 3: Implement budget and texture preflight types**

Add `RenderBudget`, defaults of 100,000 normal sprites, 20,000 debug sprites, and 256 lights, plus a private normalized copy. Add `validate_rgba_payload(width, height, rgba_len) -> Result<(), String>` with an 8192 dimension limit and a 256 MiB decoded payload limit. Keep `from_rgba`’s public return type and use the validator in `from_bytes` and `from_rgba`’s existing assertion path.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p aurora-engine renderer::tests texture::tests -- --nocapture`

Expected: all focused tests pass.

### Task 2: Enforce budgets and publish diagnostics

**Files:**
- Modify: `crates/aurora-engine/src/renderer.rs:270-330,1370-1420,1515-1610,2035-2060`
- Modify: `crates/aurora-engine/src/diagnostics.rs:10-45,90-125`
- Modify: `crates/aurora-engine/src/lib.rs:80-95`
- Test: `crates/aurora-engine/src/diagnostics.rs:100-130`

**Interfaces:**
- `Renderer::set_budget`, `Renderer::budget`, and re-exported `RenderBudget` are public.
- `DiagnosticSnapshot` includes all three dropped-work counters.

- [ ] **Step 1: Add diagnostics regression assertions**

Extend the diagnostic snapshot test with nonzero dropped counters and assert they survive `DiagnosticSnapshot::capture`.

- [ ] **Step 2: Run the diagnostic test to verify it fails**

Run: `cargo test -p aurora-engine diagnostics::tests::diagnostics_retain_raw_and_smoothed_values -- --exact`

Expected: compilation failure because the fields do not exist.

- [ ] **Step 3: Wire queue enforcement**

Store the normalized budget and dropped counters in `Renderer`. Enforce the limits in `draw_sprite`, `draw_light`, and a private debug enqueue helper used by all debug draw methods. Reset counters when a render snapshot is assembled, and preserve them in `RenderStats`.

- [ ] **Step 4: Run engine tests and clippy**

Run: `cargo test -p aurora-engine && cargo clippy -p aurora-engine --all-targets --all-features -- -D warnings`

Expected: exit 0.

