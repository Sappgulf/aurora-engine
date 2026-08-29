# Console Runtime Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Aurora a stable lifecycle, recoverable persistence, and one validated player-profile contract across native and browser builds.

**Architecture:** Add a small renderer-independent `platform` module for lifecycle state and surface status, a `profile` module for normalized settings, and keep `EngineApp` as the only platform-event adapter. Preserve existing `SaveStore::load` and `SaveStore::save` signatures while adding source-aware recovery APIs underneath them.

**Tech Stack:** Rust 2021, `winit` 0.30, `wgpu` 24, `serde`/`serde_json`, native filesystem APIs, browser `localStorage`, existing `Time`, `Input`, `Audio`, `Renderer`, and diagnostics modules.

**Spec:** `docs/superpowers/specs/2026-08-29-console-quality-foundation-design.md`

## Global Constraints

- Shared desktop and browser runtime; no vendor console SDK.
- Game policy, engine mechanics: the engine reports lifecycle and input facts; games decide whether focus loss opens a pause screen.
- Determinism is a release contract; lifecycle and profile changes must not reorder fixed-step input or mutate state-hash data.
- Bound every external input and degrade gracefully on missing devices, malformed saves, unavailable audio, and recoverable surface loss.
- Existing games keep compiling with additive defaults.
- Native save replacement must never leave both primary and backup generations unavailable.

---

### Task 1: Add the lifecycle state machine and suspend-safe time reset

**Files:**
- Create: `crates/aurora-engine/src/platform.rs`
- Modify: `crates/aurora-engine/src/lib.rs`
- Modify: `crates/aurora-engine/src/time.rs`
- Modify: `crates/aurora-engine/src/app.rs`
- Test: `crates/aurora-engine/src/platform.rs` and `crates/aurora-engine/src/time.rs` unit-test modules

**Interfaces:**
- Produces `SurfaceStatus`, `LifecycleEvent`, and `LifecycleState` for games and diagnostics.
- Adds `Game::on_lifecycle(&mut self, event: LifecycleEvent)` with a no-op default.
- Adds `Time::reset_after_suspend(&mut self)`.
- `EngineApp` owns one `LifecycleState`, emits coalesced transitions, skips simulation while suspended, and resets timing before the first resumed frame.

- [ ] **Step 1: Write the failing lifecycle tests**

```rust
#[test]
fn lifecycle_transitions_are_coalesced_and_stateful() {
    let mut state = LifecycleState::default();
    assert_eq!(state.start(), Some(LifecycleEvent::Started));
    assert_eq!(state.start(), None);
    assert_eq!(state.set_focused(false), Some(LifecycleEvent::FocusLost));
    assert_eq!(state.set_focused(false), None);
    assert_eq!(state.set_suspended(true), Some(LifecycleEvent::Suspended));
    assert!(state.suspended());
    assert_eq!(state.set_suspended(false), Some(LifecycleEvent::Resumed));
    assert!(!state.suspended());
}

#[test]
fn surface_recovery_is_distinct_from_surface_failure() {
    let mut state = LifecycleState::default();
    assert_eq!(
        state.surface_status(SurfaceStatus::Lost),
        Some(LifecycleEvent::SurfaceChanged(SurfaceStatus::Lost))
    );
    assert_eq!(
        state.surface_status(SurfaceStatus::Healthy),
        Some(LifecycleEvent::SurfaceRecovered)
    );
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run: `cargo test -p aurora-engine lifecycle_transitions_are_coalesced_and_stateful`

Expected: FAIL because `platform.rs`, `LifecycleState`, and the transition methods do not exist.

- [ ] **Step 3: Implement the minimal platform contract**

Define these exact public types in `platform.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceStatus { Healthy, Lost, Outdated, Timeout, OutOfMemory, Other }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    Started,
    Suspended,
    Resumed,
    FocusLost,
    FocusGained,
    Resized { width: u32, height: u32 },
    SurfaceChanged(SurfaceStatus),
    SurfaceRecovered,
    Terminating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleState {
    started: bool,
    focused: bool,
    suspended: bool,
    surface: SurfaceStatus,
}
```

`LifecycleState::default()` starts unfocused, unsuspended, and healthy. Add
`start`, `set_focused`, `set_suspended`, `resize`, and `surface_status` methods;
each returns `None` when the requested state is already active. Re-export the
types from `lib.rs`.

Add `Time::reset_after_suspend` to zero `delta`, `accumulator`, `alpha`, and
the per-frame fixed-step counters, then set `last = InstantCompat::now()` so a
resume cannot inject the entire suspended wall-clock interval.

- [ ] **Step 4: Wire lifecycle transitions into `EngineApp`**

Add `lifecycle: LifecycleState` and this helper:

```rust
fn emit_lifecycle(&mut self, event: LifecycleEvent) {
    if let Some(game) = self.game.as_mut() {
        game.on_lifecycle(event);
    }
}
```

Add the default `Game::on_lifecycle` hook. In `ApplicationHandler::resumed`,
emit `Resumed` when the window already exists and emit `Started` after
`on_start` on the first renderer-ready path. Implement
`ApplicationHandler::suspended` to mark suspended, emit the event, and avoid
requesting redraws. Handle `WindowEvent::Focused` and `WindowEvent::Resized`
through `LifecycleState`; retain the existing input focus-loss clearing.

Before `RedrawRequested` runs timing or game callbacks, return after clearing
the frame input when `lifecycle.suspended()` is true. On resume, call
`time.reset_after_suspend()` before the next tick. Emit `SurfaceChanged` for
`Lost`, `Outdated`, `Timeout`, `Other`, and `OutOfMemory`; emit
`SurfaceRecovered` after a successful resize/reconfigure for recoverable
errors. Keep out-of-memory exit behavior intact.

- [ ] **Step 5: Run the focused tests and verify they pass**

Run:

```bash
cargo test -p aurora-engine platform
cargo test -p aurora-engine time
```

Expected: PASS, including the existing backlog test and the new lifecycle tests.

- [ ] **Step 6: Commit the lifecycle slice**

```bash
git add crates/aurora-engine/src/platform.rs crates/aurora-engine/src/lib.rs crates/aurora-engine/src/time.rs crates/aurora-engine/src/app.rs
git commit -m "feat: add suspend-safe runtime lifecycle"
```

### Task 2: Make native and browser saves recoverable

**Files:**
- Modify: `crates/aurora-engine/src/save.rs`
- Test: `crates/aurora-engine/src/save.rs` unit-test module

**Interfaces:**
- Produces `SaveSource` and `LoadedSave<T>`.
- Adds `SaveStore::load_with_source(&self) -> Result<Option<LoadedSave<T>>, SaveError>`.
- Keeps `SaveStore::load` and `SaveStore::load_with` behavior source-compatible.
- Native backup path is `<primary>.bak`; browser backup key is `aurora:{application}:{slot}:backup`.

- [ ] **Step 1: Write failing recovery tests**

Add tests using a unique directory under `std::env::temp_dir()` and a small
serializable payload:

```rust
#[test]
fn malformed_primary_recovers_from_backup_and_reports_source() {
    let (store, path) = test_store("recovery");
    store.save(&SaveEnvelope::new(1, Payload { value: 7 })).unwrap();
    std::fs::write(&path, b"{").unwrap();

    let loaded = store.load_with_source().unwrap().unwrap();
    assert_eq!(loaded.source, SaveSource::Backup);
    assert_eq!(loaded.envelope.payload, Payload { value: 7 });
}

#[test]
fn future_versions_are_rejected_after_source_recovery_is_selected() {
    let (store, _) = test_store("future");
    store.save(&SaveEnvelope::new(9, Payload { value: 3 })).unwrap();
    assert!(matches!(
        store.load_with(4, Ok),
        Err(SaveError::NewerFormat { found: 9, supported: 4 })
    ));
}

#[test]
fn clear_removes_primary_and_backup_only_for_the_selected_slot() {
    let (store, path) = test_store("clear");
    store.save(&SaveEnvelope::new(1, Payload { value: 1 })).unwrap();
    store.save(&SaveEnvelope::new(1, Payload { value: 2 })).unwrap();
    assert!(path.with_extension("bak").exists());
    store.clear().unwrap();
    assert!(!path.exists());
    assert!(!path.with_extension("bak").exists());
}
```

- [ ] **Step 2: Run the focused recovery tests and verify failure**

Run: `cargo test -p aurora-engine malformed_primary_recovers_from_backup_and_reports_source`

Expected: FAIL because source-aware loading and backup generation do not exist.

- [ ] **Step 3: Implement source-aware reads**

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSource { Primary, Backup }

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSave<T> {
    pub envelope: SaveEnvelope<T>,
    pub source: SaveSource,
}
```

Implement `load_with_source` by reading/decoding the primary, then the backup
when the primary is missing or returns `SaveError::Serialization`. Preserve IO
errors from both locations unless a valid backup is available. Implement
`load` as `load_with_source().map(|loaded| loaded.map(|item| item.envelope))`
and make `load_with` apply the existing future-version check to the selected
envelope.

- [ ] **Step 4: Implement durable native replacement**

Use a unique temporary sibling path with process ID and a monotonic counter.
Create it with `create_new`, write the serialized bytes, call `sync_all`, and
then close it. If a primary exists, copy it to a temporary backup sibling,
flush that file, and rename the backup temporary file to `<primary>.bak`.
Replace the primary with the flushed save temporary file. On platforms where
rename-over-existing is unavailable, remove the primary only after the backup
is valid; if the final rename fails, return `SaveError::Io` and leave the
backup available. Remove abandoned temporary files best-effort after success.

Update `clear` to remove primary and backup independently, treating missing
files as success. Do not touch sibling applications or slots.

- [ ] **Step 5: Implement browser backup keys**

Before replacing the primary local-storage value, copy the current primary
string to the backup key. Set the new primary only after serialization and
backup preparation succeed. Read primary first and backup second. Remove both
keys in `clear`. Map all browser storage failures to `StorageUnavailable`.

- [ ] **Step 6: Run all save tests and commit**

Run: `cargo test -p aurora-engine save:: -- --nocapture`

Expected: PASS for primary loads, backup recovery, malformed data, future
versions, sanitization, and slot isolation.

```bash
git add crates/aurora-engine/src/save.rs
git commit -m "feat: make saves recoverable across crashes"
```

### Task 3: Add the validated engine profile

**Files:**
- Create: `crates/aurora-engine/src/profile.rs`
- Modify: `crates/aurora-engine/src/lib.rs`
- Modify: `crates/aurora-engine/src/input.rs`
- Modify: `crates/aurora-engine/src/renderer.rs`
- Modify: `crates/aurora-engine/src/audio.rs`
- Test: `crates/aurora-engine/src/profile.rs` and affected module tests

**Interfaces:**
- Produces `EngineProfile`, `AudioProfile`, `DisplayProfile`, `ControllerProfile`, and `AccessibilityProfile`.
- Adds `EngineProfile::normalized(self) -> Self` and `EngineProfile::apply(&self, input: &mut Input, audio: &mut Audio, renderer: &mut Renderer)`.
- Adds `Input::set_pad_axis_inversion(left_y: bool, right_y: bool)`.
- Adds serde support for `RenderQuality` without changing its variants.

- [ ] **Step 1: Write profile normalization tests**

```rust
#[test]
fn profile_normalization_clamps_player_values_and_preserves_intent() {
    let profile = EngineProfile {
        audio: AudioProfile { master: 2.0, music: -1.0, sfx: f32::NAN, ambience: 0.5, ui: 0.4, enabled: true },
        display: DisplayProfile { render_scale: 0.01, quality: RenderQuality::Cinematic, fullscreen: true, post_fx_enabled: true },
        controller: ControllerProfile { dead_zone: 2.0, vibration: false, invert_left_y: true, invert_right_y: false },
        accessibility: AccessibilityProfile { reduced_motion: true, screen_shake: 4.0, text_scale: 0.2, high_contrast: true },
    };
    let normalized = profile.normalized();
    assert_eq!(normalized.audio.master, 1.0);
    assert_eq!(normalized.audio.music, 0.0);
    assert_eq!(normalized.audio.sfx, 0.0);
    assert_eq!(normalized.controller.dead_zone, 0.9);
    assert_eq!(normalized.accessibility.screen_shake, 1.0);
    assert_eq!(normalized.accessibility.text_scale, 0.75);
    assert!(normalized.display.fullscreen);
}

#[test]
fn profile_round_trips_through_serde() {
    let profile = EngineProfile::default();
    let json = serde_json::to_string(&profile).unwrap();
    assert_eq!(serde_json::from_str::<EngineProfile>(&json).unwrap(), profile);
}
```

- [ ] **Step 2: Run the focused profile test and verify failure**

Run: `cargo test -p aurora-engine profile_normalization_clamps_player_values_and_preserves_intent`

Expected: FAIL because the profile types do not exist.

- [ ] **Step 3: Implement profile data and normalization**

Define public serde-enabled structs with these fields and defaults:

```rust
pub struct EngineProfile {
    pub audio: AudioProfile,
    pub display: DisplayProfile,
    pub controller: ControllerProfile,
    pub accessibility: AccessibilityProfile,
}
pub struct AudioProfile { pub master: f32, pub music: f32, pub sfx: f32, pub ambience: f32, pub ui: f32, pub enabled: bool }
pub struct DisplayProfile { pub render_scale: f32, pub quality: RenderQuality, pub fullscreen: bool, pub post_fx_enabled: bool }
pub struct ControllerProfile { pub dead_zone: f32, pub vibration: bool, pub invert_left_y: bool, pub invert_right_y: bool }
pub struct AccessibilityProfile { pub reduced_motion: bool, pub screen_shake: f32, pub text_scale: f32, pub high_contrast: bool }
```

`normalized` clamps audio and render scale to `0.0..=1.0`, dead zone to
`0.0..=0.9`, screen shake to `0.0..=1.0`, and text scale to `0.75..=2.0`;
non-finite floats become their field defaults. Derive `Serialize`,
`Deserialize`, `Clone`, `Copy`, `Debug`, and `PartialEq`.

- [ ] **Step 4: Implement profile application**

Map audio fields to the existing `AudioMixer` channels and `Audio::set_enabled`.
Map the controller dead zone and inversion flags to `Input`. Map quality to
`Renderer::set_quality`, post-processing intent to `Renderer::post_fx.enabled`,
and render scale to a new `Renderer::set_render_scale` setter that stores a
normalized presentation intent for Stage 1. Stage 2 owns any internal render
target/downsampling implementation. Leave fullscreen as an intent for the app
shell. Apply accessibility
values to public renderer post-effect settings only where an existing field is
available; keep text scale and high contrast as data for game UI.

- [ ] **Step 5: Run profile, input, audio, and renderer tests and commit**

Run:

```bash
cargo test -p aurora-engine profile
cargo test -p aurora-engine input
cargo test -p aurora-engine audio
cargo test -p aurora-engine renderer
```

Expected: PASS with the existing input/audio/renderer suites unchanged.

```bash
git add crates/aurora-engine/src/profile.rs crates/aurora-engine/src/lib.rs crates/aurora-engine/src/input.rs crates/aurora-engine/src/renderer.rs crates/aurora-engine/src/audio.rs
git commit -m "feat: add validated engine player profile"
```

### Task 4: Feed runtime status and profile recovery into diagnostics

**Files:**
- Modify: `crates/aurora-engine/src/diagnostics.rs`
- Modify: `crates/aurora-engine/src/platform.rs`
- Modify: `crates/aurora-engine/src/app.rs`
- Test: `crates/aurora-engine/src/diagnostics.rs`

**Interfaces:**
- Adds `RuntimeStatus` with `focused`, `suspended`, `surface`, `lifecycle_transitions`, `save_recoveries`, and `save_failures`.
- Adds `DiagnosticSnapshot::capture_with_runtime(...)`; retains `capture(...)` with healthy default status.
- Adds `Diagnostics::set_runtime_status` and `Diagnostics::record_save_recovery` / `record_save_failure`.

- [ ] **Step 1: Write the failing diagnostic assertions**

Extend the existing diagnostic fixture with:

```rust
let runtime = RuntimeStatus {
    focused: false,
    suspended: true,
    surface: SurfaceStatus::Lost,
    lifecycle_transitions: 4,
    save_recoveries: 1,
    save_failures: 2,
};
let snapshot = DiagnosticSnapshot::capture_with_runtime(&time, render, &assets, runtime);
assert!(snapshot.suspended);
assert_eq!(snapshot.surface, SurfaceStatus::Lost);
assert_eq!(snapshot.lifecycle_transitions, 4);
assert_eq!(snapshot.save_recoveries, 1);
assert_eq!(snapshot.save_failures, 2);
```

- [ ] **Step 2: Run the diagnostic test and verify failure**

Run: `cargo test -p aurora-engine diagnostics_retain_raw_and_smoothed_values`

Expected: FAIL because runtime fields and the capture overload do not exist.

- [ ] **Step 3: Implement runtime status capture and app wiring**

Store the current `RuntimeStatus` in `Diagnostics`. Increment the transition
counter only when `LifecycleState` returns an event. When a profile load falls
back to backup, record a recovery; when it falls back to defaults after an
error, record a failure. Capture the status alongside existing frame/render/
asset values immediately before `Diagnostics::record`.

- [ ] **Step 4: Run the complete Stage 1 gate**

Run:

```bash
cargo fmt --all -- --check
cargo test -p aurora-engine
cargo clippy -p aurora-engine --all-targets --all-features -- -D warnings
```

Expected: PASS with no change to deterministic replay tests.

- [ ] **Step 5: Commit the diagnostic integration**

```bash
git add crates/aurora-engine/src/diagnostics.rs crates/aurora-engine/src/platform.rs crates/aurora-engine/src/app.rs
git commit -m "feat: expose runtime lifecycle diagnostics"
```

### Task 5: Verify Stage 1 on native and WASM surfaces

**Files:**
- Modify: `scripts/validate-local.sh`
- Test: existing workspace test/build/browser gates

- [ ] **Step 1: Add profile/lifecycle assertions to the existing platformer and Last Light browser bridge only where deterministic state is observable**

Keep screenshots unchanged; assert that a focus-loss event clears held input,
that a subsequent resume does not move simulation time by the hidden interval,
and that malformed profile data does not prevent the canvas from rendering.

- [ ] **Step 2: Run the complete validator**

Run: `./scripts/validate-local.sh`

Expected: native tests, strict WASM checks, both release builds, all browser
tests, exact replay validation, and artifact budgets pass.

- [ ] **Step 3: Commit only Stage 1 test/script changes**

```bash
git add scripts/validate-local.sh playtests/browser
git commit -m "test: verify console runtime recovery paths"
```
