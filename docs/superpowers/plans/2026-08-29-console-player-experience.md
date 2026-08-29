# Console Player Experience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make menus and settings feel console-native through semantic controller actions, repeat-aware navigation, persistent profile application, and accessibility-safe feedback.

**Architecture:** Extend `InputMap` with device-neutral bindings while preserving `bind_key`, add a pure repeat gate consumed by the existing renderer-agnostic `MenuState`, and let `EngineApp` own profile loading/application/persistence. Games continue owning labels, layouts, and game-specific settings.

**Tech Stack:** Rust 2021, existing `Input`, `InputMap`, `PadButton`, `MenuState`, `MenuInput`, `SaveStore`, `EngineProfile`, `Audio`, and browser/native agent bridges.

**Spec:** `docs/superpowers/specs/2026-08-29-console-quality-foundation-design.md`

## Global Constraints

- Menus consume semantic actions, never physical keys directly.
- Edge-triggered actions fire once; held directional input repeats only after an initial delay.
- Focus loss and controller disconnect reset repeat state and clear held input.
- Haptics respect `EngineProfile.controller.vibration`.
- Settings preview/cancel/apply is transactional in memory.
- Existing `MenuState::handle` and keyboard bindings remain source-compatible.
- Text, art, layout, and game-specific copy remain owned by each game.

---

### Task 1: Extend `InputMap` to keyboard and gamepad actions

**Files:**
- Modify: `crates/aurora-engine/src/input.rs`
- Modify: `crates/aurora-engine/src/lib.rs`
- Test: `crates/aurora-engine/src/input.rs`

**Interfaces:**
- Produces `ActionBinding::{Key, Pad}`.
- `InputMap::bind_key` remains unchanged; add `InputMap::bind_pad(action, slot: Option<usize>, button: PadButton)`.
- `Input::action_down` and `Input::action_pressed` evaluate both binding kinds.
- Adds `Input::navigation_axis(slot: Option<usize>) -> Vec2`.

- [ ] **Step 1: Write failing semantic-binding tests**

```rust
#[test]
fn semantic_actions_read_keyboard_and_gamepad_bindings() {
    let mut input = Input::new();
    let action = ActionId::new("menu.confirm");
    let mut map = InputMap::default();
    map.bind_key(action.clone(), KeyBinding::key(KeyCode::Enter));
    map.bind_pad(action.clone(), None, PadButton::South);

    input.simulate_gamepad_button(0, PadButton::South, true);
    assert!(input.action_pressed(&map, &action));
}

#[test]
fn navigation_axis_prefers_dpad_and_resets_after_disconnect() {
    let mut input = Input::new();
    let frame = GamepadFrame {
        connected: true,
        left_stick: Vec2::new(-0.8, 0.0),
        buttons: {
            let mut buttons = [false; 16];
            buttons[PadButton::DpadRight.index()] = true;
            buttons
        },
        ..Default::default()
    };
    input.push_gamepad_frame(0, &frame);
    assert_eq!(input.navigation_axis(Some(0)), Vec2::X);
    input.clear_gamepads();
    assert_eq!(input.navigation_axis(Some(0)), Vec2::ZERO);
}
```

- [ ] **Step 2: Run the focused tests and verify failure**

Run: `cargo test -p aurora-engine semantic_actions_read_keyboard_and_gamepad_bindings`

Expected: FAIL because `ActionBinding`, `bind_pad`, and `navigation_axis` do not exist.

- [ ] **Step 3: Implement additive action bindings**

Change `InputMap` storage to `HashMap<ActionId, Vec<ActionBinding>>`. Keep
`bind_key` constructing `ActionBinding::Key`. `bind_pad` stores an optional
slot, where `None` means the first connected pad. Make `bindings` private to
the module and update `Input::action_down` / `action_pressed` to match the
binding variant. An action is pressed when any matching keyboard or pad edge is
visible under the existing fixed-step edge policy.

- [ ] **Step 4: Implement analog menu navigation**

`navigation_axis` reads the selected pad's left stick after the configured
radial dead zone. If any D-pad direction is held, return the corresponding
cardinal vector instead of the stick, matching `move_axis` precedence. Return
zero for an absent/disconnected slot and normalize non-finite values.

- [ ] **Step 5: Run input tests and commit**

Run: `cargo test -p aurora-engine input::`

Expected: PASS, including fixed-step edge, focus-loss, rumble, dead-zone, and
the new semantic binding tests.

```bash
git add crates/aurora-engine/src/input.rs crates/aurora-engine/src/lib.rs
git commit -m "feat: add device-neutral action bindings"
```

### Task 2: Add repeat-aware semantic menu navigation

**Files:**
- Modify: `crates/aurora-engine/src/ui.rs`
- Test: `crates/aurora-engine/src/ui.rs`

**Interfaces:**
- Produces `MenuNavigator` with `new`, `poll`, and `reset`.
- `MenuNavigator::poll(&mut self, direction: Option<MenuInput>, delta: f32) -> Option<MenuInput>` returns one semantic event per call.
- Existing `MenuState::handle` remains the one-shot state transition API.

- [ ] **Step 1: Write repeat-gate tests**

```rust
#[test]
fn menu_navigation_repeats_after_initial_delay() {
    let mut navigator = MenuNavigator::new(0.35, 0.10);
    assert_eq!(navigator.poll(Some(MenuInput::Down), 0.0), Some(MenuInput::Down));
    assert_eq!(navigator.poll(Some(MenuInput::Down), 0.34), None);
    assert_eq!(navigator.poll(Some(MenuInput::Down), 0.01), Some(MenuInput::Down));
    assert_eq!(navigator.poll(Some(MenuInput::Down), 0.09), None);
    assert_eq!(navigator.poll(Some(MenuInput::Down), 0.01), Some(MenuInput::Down));
}

#[test]
fn changing_direction_and_release_reset_repeat_state() {
    let mut navigator = MenuNavigator::new(0.35, 0.10);
    assert_eq!(navigator.poll(Some(MenuInput::Down), 0.0), Some(MenuInput::Down));
    assert_eq!(navigator.poll(Some(MenuInput::Up), 0.0), Some(MenuInput::Up));
    assert_eq!(navigator.poll(None, 1.0), None);
    assert_eq!(navigator.poll(Some(MenuInput::Up), 0.0), Some(MenuInput::Up));
}
```

- [ ] **Step 2: Run focused tests and verify failure**

Run: `cargo test -p aurora-engine menu_navigation_repeats_after_initial_delay`

Expected: FAIL because `MenuNavigator` does not exist.

- [ ] **Step 3: Implement the bounded repeat policy**

Store the current direction, elapsed hold time, initial delay, and repeat
interval. Normalize invalid constructor values to `initial_delay >= 0.1` and
`repeat_interval >= 0.03`. A new direction emits immediately; the same held
direction emits after the initial delay and once per interval; `None` clears
state. Consume large deltas with a bounded loop of at most eight repeats per
call so a hitch cannot flood a menu with events.

- [ ] **Step 4: Add focus/disconnect reset coverage**

Call `MenuNavigator::reset` from game menu code when `Input` reports focus loss
or no connected pad. Keep the engine primitive independent of window events;
the caller supplies `None` on release/disconnect.

- [ ] **Step 5: Run UI tests and commit**

Run: `cargo test -p aurora-engine ui::`

Expected: PASS with existing title/pause/results behavior unchanged.

```bash
git add crates/aurora-engine/src/ui.rs
git commit -m "feat: add repeat-aware menu navigation"
```

### Task 3: Load, apply, and persist the engine profile

**Files:**
- Modify: `crates/aurora-engine/src/app.rs`
- Modify: `crates/aurora-engine/src/profile.rs`
- Modify: `crates/aurora-engine/src/save.rs`
- Modify: `crates/aurora-engine/src/ui.rs`
- Test: `crates/aurora-engine/src/app.rs` pure helpers and profile/save tests

**Interfaces:**
- `FrameCtx` gains `profile: &mut EngineProfile`.
- `Game` gains `on_profile_loaded(&mut self, profile: &mut EngineProfile, is_new: bool)` with a no-op default.
- `run_result` loads `SaveStore<EngineProfile>` using application `aurora-engine`, the sanitized game name as slot, and profile format version `1`.
- `EngineApp` reapplies and persists a normalized profile only when its value changes.

- [ ] **Step 1: Write the profile persistence tests**

```rust
#[test]
fn profile_changes_are_normalized_before_persistence() {
    let mut profile = EngineProfile::default();
    profile.audio.master = f32::NAN;
    profile.accessibility.text_scale = 9.0;
    let normalized = profile.normalized();
    assert_eq!(normalized.audio.master, 1.0);
    assert_eq!(normalized.accessibility.text_scale, 2.0);
}

#[test]
fn profile_defaults_can_preserve_legacy_game_intent() {
    let mut profile = EngineProfile::default();
    profile.display.post_fx_enabled = true;
    profile.accessibility.reduced_motion = true;
    assert!(profile.display.post_fx_enabled);
    assert!(profile.accessibility.reduced_motion);
}
```

- [ ] **Step 2: Run focused tests and verify the integration API is absent**

Run: `cargo test -p aurora-engine profile_changes_are_normalized_before_persistence`

Expected: the normalization and default-intent assertions pass from Stage 1;
the app integration remains unimplemented until the following steps.

- [ ] **Step 3: Add profile ownership to the app shell**

Add `profile: EngineProfile` and `profile_store: SaveStore<EngineProfile>` to
`EngineApp`. Load it before constructing the app; if the primary recovers from
backup, use the recovered envelope and record the diagnostic. If decoding or
storage fails, use `EngineProfile::default()` and record one failure. Apply the
profile to input/audio immediately and to the renderer after `RendererReady`.
Call `game.on_profile_loaded` after the initial application, passing `is_new`
when no valid engine profile was selected. This lets a game migrate legacy
settings exactly once without overwriting an already-persisted engine profile.

- [ ] **Step 4: Make profile edits transactional in `FrameCtx`**

Add the mutable profile reference to every fixed/update/post-update context.
Before callbacks copy the current profile; after callbacks normalize it, compare
with the previous value, apply changes to live resources, and save the new
envelope only when different. A failed save leaves the in-memory profile
active, records a failure, and does not roll back unrelated game simulation.

- [ ] **Step 5: Add settings preview/apply/cancel primitives**

Add `SettingsTransaction` in `ui.rs` with `begin(profile)`, `preview(profile)`,
`apply() -> EngineProfile`, and `cancel() -> EngineProfile`. Tests must prove
cancel returns the original profile and apply returns the normalized preview.
Keep `MenuState` commands compatible; games decide which rows use the
transaction.

- [ ] **Step 6: Migrate Aurora Run settings without breaking old saves**

Add an optional `EngineProfile` field to `games/aurora-run/src/save.rs` with
`#[serde(default)]`. When `is_new` is true, seed a missing game profile from
the existing `post_fx_enabled` and `reduced_motion` flags; persist the migrated
profile alongside the legacy fields. Update settings actions to edit the
profile reference and retain the legacy flags until their save migration is
proven by tests. Do not change Last Light campaign payload fields.

- [ ] **Step 7: Run engine and game tests and commit**

Run:

```bash
cargo test -p aurora-engine
cargo test -p aurora_run
cargo test -p last_light
```

Expected: PASS, including old Aurora Run saves, Last Light campaign saves, and
all replay tests.

```bash
git add crates/aurora-engine/src/app.rs crates/aurora-engine/src/profile.rs crates/aurora-engine/src/save.rs crates/aurora-engine/src/ui.rs games/aurora-run/src/save.rs games/aurora-run/src/main.rs
git commit -m "feat: persist and apply console player profiles"
```

### Task 4: Make feedback accessibility-safe

**Files:**
- Modify: `crates/aurora-engine/src/input.rs`
- Modify: `crates/aurora-engine/src/audio.rs`
- Modify: `crates/aurora-engine/src/juice.rs`
- Modify: `games/aurora-run/src/main.rs`
- Test: affected module tests

- [ ] **Step 1: Add vibration and motion-policy tests**

Assert that disabled vibration emits no queued rumble, reduced motion clamps
screen shake to zero, and text scale normalization remains bounded.

- [ ] **Step 2: Apply policy at request boundaries**

Give `Input` a vibration-enabled flag used by `rumble` and `rumble_first`.
Give the shared motion helper a normalized intensity multiplier; callers pass
the profile's reduced-motion value rather than duplicating conditionals.
Preserve existing default behavior for profiles with vibration enabled and
reduced motion disabled.

- [ ] **Step 3: Add controller disconnect/reconnect messaging hooks**

Expose a bounded `Input::connected_pad_count` and let menu consumers detect a
transition from connected to disconnected. Do not synthesize gameplay presses
on reconnect. Reset repeat and rumble queues on focus loss.

- [ ] **Step 4: Run focused tests and commit**

Run:

```bash
cargo test -p aurora-engine input
cargo test -p aurora-engine audio
cargo test -p aurora-engine juice
cargo test -p aurora_run
```

Expected: PASS.

```bash
git add crates/aurora-engine/src/input.rs crates/aurora-engine/src/audio.rs crates/aurora-engine/src/juice.rs games/aurora-run/src/main.rs
git commit -m "feat: respect accessibility and device recovery policies"
```

### Task 5: Verify controller-first browser/native UX

**Files:**
- Modify: `playtests/browser/agent-control.spec.mjs`
- Modify: `playtests/browser/last-light.spec.mjs`
- Modify: `scripts/validate-local.sh`

- [ ] **Step 1: Add browser bridge tests for semantic menu actions**

Use the existing injected pad bridge to confirm South confirms, D-pad Down
moves once, held Down repeats after the authored delay, and focus loss clears
the held action.

- [ ] **Step 2: Run the complete validation gate**

Run: `./scripts/validate-local.sh`

Expected: native tests, strict WASM checks, both release builds, replay checks,
all browser tests, and artifact budgets pass.

- [ ] **Step 3: Commit Stage 3 verification**

```bash
git add playtests/browser/agent-control.spec.mjs playtests/browser/last-light.spec.mjs scripts/validate-local.sh
git commit -m "test: verify controller-first console UX"
```
