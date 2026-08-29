# Console Performance Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make presentation quality measurable and adaptive while preserving fixed-step simulation, replay hashes, and native/WASM parity.

**Architecture:** Add a pure `QualityController` driven by synthetic frame samples, return renderer queue ownership after each frame so capacities survive, and extend the deterministic asset queue with priority and residency accounting. `EngineApp` applies presentation changes only after diagnostics are recorded; replay/capture callers can disable adaptation.

**Tech Stack:** Rust 2021, existing `wgpu` renderer, `RenderBudget`, `RenderQuality`, `SpriteBatch`, `AssetLoadQueue`, `Diagnostics`, and `Time`.

**Spec:** `docs/superpowers/specs/2026-08-29-console-quality-foundation-design.md`

## Global Constraints

- Adaptive quality never changes `Time::fixed_dt`, input semantics, simulation order, or state-hash data.
- Synthetic frame samples drive deterministic controller tests; wall-clock measurements affect presentation only.
- Existing renderer admission limits remain hard limits.
- Optional presentation work is rejected before gameplay-critical work.
- No render-graph or ECS rewrite is part of this plan.
- Native and browser builds must share the same quality policy and diagnostics fields.

---

### Task 1: Add deterministic adaptive-quality hysteresis

**Files:**
- Create: `crates/aurora-engine/src/performance.rs`
- Modify: `crates/aurora-engine/src/lib.rs`
- Modify: `crates/aurora-engine/src/renderer.rs`
- Test: `crates/aurora-engine/src/performance.rs`

**Interfaces:**
- Produces `QualityController` and `QualityControllerConfig`.
- `QualityController::observe(&mut self, frame_ms: f32) -> Option<RenderQuality>` returns only when quality changes.
- `QualityController::set_enabled`, `enabled`, `quality`, and `reset` are public.

- [ ] **Step 1: Write failing hysteresis tests**

```rust
#[test]
fn quality_drops_after_three_over_budget_samples() {
    let mut controller = QualityController::new(16.67);
    controller.set_quality(RenderQuality::Cinematic);
    assert_eq!(controller.observe(19.5), None);
    assert_eq!(controller.observe(19.5), None);
    assert_eq!(controller.observe(19.5), Some(RenderQuality::Balanced));
}

#[test]
fn quality_recovers_only_after_a_long_under_budget_window() {
    let mut controller = QualityController::new(16.67);
    controller.set_quality(RenderQuality::Performance);
    for _ in 0..119 { assert_eq!(controller.observe(10.0), None); }
    assert_eq!(controller.observe(10.0), Some(RenderQuality::Balanced));
}

#[test]
fn invalid_samples_and_disabled_adaptation_are_side_effect_free() {
    let mut controller = QualityController::new(16.67);
    controller.set_enabled(false);
    assert_eq!(controller.observe(f32::NAN), None);
    assert_eq!(controller.quality(), RenderQuality::Balanced);
}
```

- [ ] **Step 2: Run the focused tests and verify failure**

Run: `cargo test -p aurora-engine quality_drops_after_three_over_budget_samples`

Expected: FAIL because `performance.rs` and `QualityController` do not exist.

- [ ] **Step 3: Implement the pure controller**

Use a default target of `16.67` ms, an over-budget threshold of `target * 1.15`,
an under-budget threshold of `target * 0.80`, three degradation samples, and
120 recovery samples. Ignore non-finite or non-positive samples. Degradation
maps Cinematic → Balanced → Performance; recovery maps Performance → Balanced
→ Cinematic. Reset the opposite streak after every sample and reset both
streaks after a quality change. `set_quality` resets streaks.

- [ ] **Step 4: Make `RenderQuality` serializable and re-export the controller**

Add serde derives to the existing enum without changing variant names or
ordering, re-export `QualityController` and its config from `lib.rs`, and keep
the renderer's current light-budget mapping unchanged.

- [ ] **Step 5: Run focused tests and commit**

Run:

```bash
cargo test -p aurora-engine performance
cargo test -p aurora-engine quality_presets_use_bounded_light_budgets
```

Expected: PASS.

```bash
git add crates/aurora-engine/src/performance.rs crates/aurora-engine/src/lib.rs crates/aurora-engine/src/renderer.rs
git commit -m "feat: add deterministic adaptive quality controller"
```

### Task 2: Retain renderer queue capacity and expose throughput counters

**Files:**
- Modify: `crates/aurora-engine/src/renderer.rs`
- Modify: `crates/aurora-engine/src/sprite.rs`
- Modify: `crates/aurora-engine/src/diagnostics.rs`
- Test: `crates/aurora-engine/src/renderer.rs` and `crates/aurora-engine/src/diagnostics.rs`

**Interfaces:**
- `RenderStats` gains `staged_vertices`, `staged_indices`, `sprite_upload_bytes`, and `quality`.
- The renderer retains draw/light queue capacity after `render`.
- No public sprite ordering behavior changes.

- [ ] **Step 1: Write the capacity and counter tests**

Add and test this renderer-local helper around queue ownership:

```rust
fn take_with_capacity<T>(queue: &mut Vec<T>) -> Vec<T> {
    let capacity = queue.capacity();
    std::mem::replace(queue, Vec::with_capacity(capacity))
}

#[test]
fn returned_queue_keeps_capacity_after_staging() {
    let mut queue = Vec::with_capacity(128);
    queue.extend((0..32).map(|_| 1_u32));
    let capacity = queue.capacity();
    let mut work = take_with_capacity(&mut queue);
    work.clear();
    queue = work;
    assert_eq!(queue.capacity(), capacity);
}
```

Extend renderer admission/staging tests to assert that staged vertices equal
`drawn_sprites * 4`, indices equal `drawn_sprites * 6`, and upload bytes are
finite and nonzero when sprites are drawn.

- [ ] **Step 2: Run focused tests and verify the new counter fields fail to compile**

Run: `cargo test -p aurora-engine returned_queue_keeps_capacity_after_staging`

Expected: the helper test passes only after the queue return path exists; the
counter assertions fail until `RenderStats` is extended.

- [ ] **Step 3: Return ownership of queue vectors after render**

Replace `std::mem::take` for `draw_queue` and `light_queue` with a capacity-
preserving swap. Process the work vectors, clear them after staging, then
assign them back to the renderer before returning from `render`, including the
surface-error path where the frame cannot be presented. Keep debug queue and
stage buffers retained as they already are.

- [ ] **Step 4: Record staging and upload counters**

Populate the new fields directly after staging. Count bytes using
`std::mem::size_of::<SpriteVertex>()` and `size_of::<u32>()` with checked
multiplication. Add `quality: self.quality` to the stats snapshot and expose
the values through `DiagnosticSnapshot`.

- [ ] **Step 5: Run renderer and diagnostics tests and commit**

Run:

```bash
cargo test -p aurora-engine renderer
cargo test -p aurora-engine diagnostics
```

Expected: PASS with all existing budget/culling/order tests.

```bash
git add crates/aurora-engine/src/renderer.rs crates/aurora-engine/src/sprite.rs crates/aurora-engine/src/diagnostics.rs
git commit -m "perf: retain render queues and expose throughput metrics"
```

### Task 3: Add deterministic asset priority and residency accounting

**Files:**
- Modify: `crates/aurora-engine/src/loader.rs`
- Modify: `crates/aurora-engine/src/diagnostics.rs`
- Test: `crates/aurora-engine/src/loader.rs`

**Interfaces:**
- Produces `AssetPriority::{Critical, Gameplay, Optional}`.
- Adds `AssetLoadQueue::enqueue_with_priority`, `pending_count`, `set_resident_bytes`, `resident_bytes`, and `admit_resident_bytes`.
- `begin_next` selects lower priority number first, then stable `AssetKey` order.

- [ ] **Step 1: Write priority and budget tests**

```rust
#[test]
fn asset_priority_is_deterministic_and_optional_work_is_rejected_first() {
    let mut queue = AssetLoadQueue::default();
    let optional = AssetKey::new("optional.cover").unwrap();
    let critical = AssetKey::new("critical.player").unwrap();
    queue.enqueue_with_priority(optional.clone(), AssetPriority::Optional);
    queue.enqueue_with_priority(critical.clone(), AssetPriority::Critical);
    assert_eq!(queue.begin_next(), Some(critical));
    assert_eq!(queue.pending_count(), 1);
}

#[test]
fn residency_budget_never_exceeds_the_configured_bytes() {
    let mut queue = AssetLoadQueue::default();
    queue.set_residency_budget(1024);
    let gameplay = AssetKey::new("gameplay.hero").unwrap();
    let optional = AssetKey::new("optional.fx").unwrap();
    queue.enqueue(gameplay.clone());
    queue.enqueue_with_priority(optional.clone(), AssetPriority::Optional);
    assert!(queue.admit_resident_bytes(&gameplay, 768, false));
    assert!(!queue.admit_resident_bytes(&optional, 512, true));
    assert_eq!(queue.resident_bytes(), 768);
}
```

- [ ] **Step 2: Run the focused loader tests and verify failure**

Run: `cargo test -p aurora-engine asset_priority_is_deterministic_and_optional_work_is_rejected_first`

Expected: FAIL because priorities and residency methods do not exist.

- [ ] **Step 3: Implement priority ordering without nondeterministic maps**

Derive `Ord` and `PartialOrd` for `AssetPriority`. Add priority to each entry
and store pending keys in a `BTreeSet<(AssetPriority, AssetKey)>` or equivalent
ordered structure. Keep `AssetLoadQueue::enqueue` as
the gameplay-priority wrapper. `begin_next` removes the first ordered key and
marks it Loading. Re-enqueueing an existing key remains false.

- [ ] **Step 4: Implement residency admission and counters**

Track a configurable byte budget, current resident bytes, and rejected
optional bytes. `admit_resident_bytes` accepts critical/gameplay work only when
it fits; optional work is rejected when it would exceed the budget. Replacing
resident bytes for a key subtracts the old value before checking the new one.
Expose the counters through diagnostics; never make a failed optional
admission fatal.

- [ ] **Step 5: Run loader/diagnostic tests and commit**

Run:

```bash
cargo test -p aurora-engine loader
cargo test -p aurora-engine diagnostics
```

Expected: PASS, including the existing synchronous-manifest behavior.

```bash
git add crates/aurora-engine/src/loader.rs crates/aurora-engine/src/diagnostics.rs
git commit -m "feat: prioritize and budget asset residency"
```

### Task 4: Integrate adaptive presentation without touching simulation

**Files:**
- Modify: `crates/aurora-engine/src/app.rs`
- Modify: `crates/aurora-engine/src/diagnostics.rs`
- Modify: `crates/aurora-engine/src/performance.rs`
- Test: `crates/aurora-engine/src/app.rs` where pure helpers are available

**Interfaces:**
- Adds `Game::adaptive_quality(&self) -> bool` with default `true`.
- `EngineApp` owns a `QualityController` and applies a returned quality only after render statistics are captured.
- A game returning `false` keeps its configured renderer quality fixed.

- [ ] **Step 1: Write the opt-out test**

Add a pure controller integration assertion:

```rust
#[test]
fn disabled_adaptive_quality_never_changes_the_selected_tier() {
    let mut controller = QualityController::new(16.67);
    controller.set_quality(RenderQuality::Cinematic);
    controller.set_enabled(false);
    for _ in 0..10 { assert_eq!(controller.observe(40.0), None); }
    assert_eq!(controller.quality(), RenderQuality::Cinematic);
}
```

- [ ] **Step 2: Implement app integration**

Initialize the controller at Balanced. On each rendered frame, capture the
current `RenderStats` first, record diagnostics, then if the game allows
adaptation call `observe(stats.cpu_frame_ms)` and apply a returned tier with
`renderer.set_quality`. Never call `observe` from fixed update or before the
state hash is produced. On suspend/resume call `controller.reset()` so a stale
pre-suspend streak cannot immediately change quality.

- [ ] **Step 3: Run native, replay, and strict WASM checks**

Run:

```bash
cargo test -p aurora-engine
cargo test -p last_light --lib simulation::tests::reclaim_truth_trace_replays_through_victory_with_the_same_hash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS; replay hashes remain identical because only presentation
settings change.

- [ ] **Step 4: Commit the integration**

```bash
git add crates/aurora-engine/src/app.rs crates/aurora-engine/src/diagnostics.rs crates/aurora-engine/src/performance.rs
git commit -m "perf: adapt presentation quality from frame headroom"
```

### Task 5: Verify Stage 2 budgets and browser parity

**Files:**
- Modify: `scripts/validate-local.sh`
- Modify: `scripts/check-web-budget.sh`
- Test: `playtests/browser/last-light.spec.mjs`

- [ ] **Step 1: Add deterministic asset-pressure and renderer-counter checks to the browser agent state**

Expose only bounded integer counters through the existing diagnostics bridge;
do not expose raw GPU handles or unbounded queues.

- [ ] **Step 2: Run the full validator**

Run: `./scripts/validate-local.sh`

Expected: all native tests, strict WASM checks, release builds, 15 browser
tests, replay validation, and all artifact budgets pass.

- [ ] **Step 3: Commit the Stage 2 verification changes**

```bash
git add scripts/validate-local.sh scripts/check-web-budget.sh playtests/browser/last-light.spec.mjs
git commit -m "test: verify console presentation budgets"
```
