# Simulation Scale Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the RTS unit update’s all-units interaction scan while preserving deterministic motion and state-hash behavior.

**Architecture:** The existing deterministic `SpatialUnitIndex` becomes the broadphase for unit interactions. At the start of `RtsWorld::update`, it indexes the previous tick’s finite living positions; each unit queries a reusable candidate buffer, candidates are sorted back into snapshot order, and the existing separation math receives the same logical neighbor sequence. Attack/follow lookup uses a snapshot map instead of a linear scan.

**Tech Stack:** Rust, glam, existing `HashMap`/`HashSet` spatial index.

**Spec:** `docs/superpowers/specs/2026-08-29-aurora-evolution-design.md`

## Global Constraints

- Preserve current public movement/combat APIs.
- Preserve deterministic candidate order and floating-point accumulation order.
- Keep the existing selection spatial index behavior unchanged.
- Do not add threads, nondeterministic jobs, or a new dependency.

---

### Task 1: Prove spatial query ordering and finite guards

**Files:**
- Modify: `crates/aurora-engine/src/rts.rs:1035-1115`
- Test: `crates/aurora-engine/src/rts.rs` spatial/index tests

**Interfaces:**
- Produces `SpatialUnitIndex::query_cell_ids_into(&self, position: Vec2, radius: f32, ids: &mut Vec<UnitId>)`.
- `SpatialUnitIndex::build` ignores non-finite positions.

- [ ] **Step 1: Write failing tests**

Add a test that queries a multi-cell index into a reusable vector and asserts the same IDs as the allocating query. Add a test that a NaN position is not indexed and a non-finite query returns an empty vector.

- [ ] **Step 2: Run focused tests to verify failure**

Run: `cargo test -p aurora-engine rts::tests -- --nocapture`

Expected: compilation failure because the into-query method is missing and current non-finite behavior is not guarded.

- [ ] **Step 3: Implement the reusable query**

Clear the caller-provided vector, return early for non-finite position/radius, iterate the same cell ranges as `query_cell_ids`, extend bucket IDs, and retain the infinity behavior by collecting `alive_units` in sorted ID order. Make the existing allocating method delegate to the new method.

- [ ] **Step 4: Run focused RTS tests**

Run: `cargo test -p aurora-engine rts::tests -- --nocapture`

Expected: all RTS tests pass.

### Task 2: Replace quadratic unit neighbor and target scans

**Files:**
- Modify: `crates/aurora-engine/src/rts.rs:2585-2795`
- Test: `crates/aurora-engine/src/rts.rs` movement tests

**Interfaces:**
- `RtsWorld::update` retains its signature and output behavior.
- `separation_impulse` continues to consume `&[(UnitId, Vec2, f32)]` in deterministic order.

- [ ] **Step 1: Add a deterministic equivalence test**

Create a world with several units in near and far cells, configure separation, run one update, and assert that near units separate while a far unit remains outside the interaction radius. Add attack/follow targets to assert target resolution remains valid.

- [ ] **Step 2: Run the new test before implementation**

Run: `cargo test -p aurora-engine rts::tests -- --nocapture`

Expected: the behavioral assertions pass against the old implementation, establishing the contract before changing the broadphase.

- [ ] **Step 3: Implement the indexed snapshot**

Rebuild the spatial index if dirty before taking the tick snapshot. Build `neighbor_positions: HashMap<UnitId, (Vec2, f32)>` and `neighbor_order: HashMap<UnitId, usize>`. For each unit, query a radius covering its own radius, separation radius, and the maximum snapshot radius into one reusable ID buffer, sort IDs by `neighbor_order`, and build the local candidate slice. Resolve `Attack` and `Follow` from `neighbor_positions`. Keep obstacle loops and all motion calculations unchanged.

- [ ] **Step 4: Run RTS tests and trace tests**

Run: `cargo test -p aurora-engine rts::tests && cargo test -p last_light simulation::tests::reclaim_truth_trace_replays_through_victory_with_the_same_hash -- --exact`

Expected: all selected tests pass and the deterministic Last Light trace hash is unchanged.

### Task 3: Verify scale behavior and whole workspace

**Files:**
- Modify: `crates/aurora-engine/src/diagnostics.rs` only if a new counter is required by the measured test.
- Test: `crates/aurora-engine/src/rts.rs`

- [ ] **Step 1: Add a candidate-count test hook**

Exercise `query_cell_ids_into` with a large sparse index and assert the result count stays bounded by nearby buckets rather than the total unit count.

- [ ] **Step 2: Run all workspace verification**

Run: `cargo fmt --all -- --check && cargo test --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: exit 0 with no warnings.

