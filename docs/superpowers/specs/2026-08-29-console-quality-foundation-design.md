# Aurora Engine Console-Quality Foundation

**Date:** 2026-08-29  
**Status:** Draft for review  
**Scope:** Shared desktop and browser runtime; no vendor console SDK

## Goal

Raise Aurora from a strong prototype engine to a console-quality runtime
contract: predictable lifecycle behavior, resilient persistence, stable frame
pacing, bounded performance, controller-first interaction, and player-facing
settings/accessibility. The work must preserve the engine's current strengths:
deterministic fixed-step simulation, native/WASM parity, game-owned rules and
saves, and renderer-independent UI primitives.

"Console quality" here means the behavior and reliability players expect from
a polished game. It does not claim PlayStation, Xbox, or Switch certification;
vendor SDK integration remains a separate future port.

## Current baseline and gaps

Aurora already provides bounded fixed-step time, gamepad snapshots and rumble,
logical keyboard bindings, versioned save envelopes, atomic native writes,
browser storage, renderer admission budgets, quality tiers, menu primitives,
and replay/browser validation.

The remaining product gaps are cross-system contracts rather than isolated
features:

1. `EngineApp` handles surface errors but does not expose a complete lifecycle
   contract to games or explicitly distinguish suspension from focus loss.
2. Native saves use a single temporary filename and do not recover from a
   partially completed replacement; browser saves have no backup generation.
3. `InputMap` stores keyboard bindings but does not yet provide a complete
   semantic action query surface or repeat-aware controller navigation.
4. Settings are implemented independently by games, so audio, display,
   controller, and accessibility behavior cannot be applied consistently.
5. Frame and asset diagnostics expose useful counters but do not identify
   lifecycle transitions, budget misses, save failures, or device state.
6. The renderer is bounded, but quality selection and asset work are mostly
   static rather than driven by measured frame headroom.

## Design principles

- **Game policy, engine mechanics.** The engine reports lifecycle and input
  facts; games decide whether a focus loss opens a pause screen or suppresses a
  particular activity.
- **Determinism is a release contract.** Performance changes may reduce work,
  but cannot reorder simulation inputs, state-hash data, or authored content.
- **Bound every external input.** Files, browser storage, controllers, assets,
  render queues, and telemetry all receive explicit size and failure limits.
- **Degrade gracefully.** A missing device, malformed save, unavailable audio
  backend, or lost surface should produce a recoverable state and a diagnostic,
  not a panic or a wedged game loop.
- **Additive public APIs.** Existing games keep compiling with defaults; new
  behavior is opt-in where changing policy would be surprising.

## Stage 1 — Runtime foundation

### Lifecycle contract

Add a renderer-agnostic lifecycle event type and an additive `Game` hook. The
app shell emits stable transitions for startup, focus loss/gain, suspension /
resume, resize, surface recovery, and terminal GPU failure. Repeated platform
events are coalesced so a game sees one transition per state change.

The shell must:

- clear held input on focus loss;
- stop fixed-step simulation while the application is suspended;
- reset stale frame delta and fixed-step backlog on resume;
- continue safe rendering or request redraws when the surface is recoverable;
- record surface errors and lifecycle transitions in diagnostics;
- leave the policy for showing a pause menu to the game.

Existing `Lost` and `Outdated` surface recovery remains the low-level mechanism;
this stage gives games and tests a stable signal around it.

### Durable persistence

Harden `SaveStore` without taking ownership of game payloads:

- write a uniquely named temporary file, flush it, and use a platform-safe
  replacement sequence on native platforms;
- retain one validated backup generation;
- load primary first, then backup when primary is missing or malformed;
- use namespaced primary and backup keys in browser storage;
- expose whether a load recovered from backup;
- keep future-format rejection and game-owned migration behavior unchanged.

The native replacement sequence must never leave both generations unavailable:
the existing primary is preserved as the backup before replacement, the new
temporary file is flushed before it becomes primary, and any failed rename or
restore is reported without deleting the last-known-good bytes. All recovery
paths remain bounded by the existing serialization limits. Clear operations
affect only the resolved application/slot pair.

### Profile and settings contract

Add a serializable, validated engine profile containing:

- master/music/SFX volume;
- display scale, fullscreen intent, and post-processing quality;
- controller dead zone, vibration toggle, and axis preferences;
- reduced motion, screen-shake intensity, text scale, and high-contrast intent.

Invalid or missing fields normalize to safe defaults. The profile is data only;
the app shell and games apply it to `Audio`, `Input`, `Renderer`, and their own
menus. Settings changes are transactional in memory and persisted only after a
successful validation pass.

## Stage 2 — Performance foundation

### Frame pacing and budgets

Extend diagnostics with target frame time, observed frame time, simulation
overrun/discard counts, render-budget misses, asset queue pressure, and surface
status. Add a bounded quality controller with a deterministic hysteresis policy
that can move between existing quality tiers, with an explicit opt-out for
replay and capture tests.

The controller must never change simulation `fixed_dt`, input semantics, or
state-hash behavior. It only changes presentation work such as post effects,
light count, shadow resolution, and asset residency.

### Render and asset throughput

Keep sprite admission bounded while reducing avoidable CPU/GPU overhead:

- reuse per-frame staging buffers and sort scratch storage;
- group compatible sprite work without changing z-order semantics;
- expose batch and upload counters separately from draw-call counts;
- make asset loading incremental and priority-aware rather than marking an
  entire manifest ready as one opaque step;
- enforce decoded texture and total residency budgets;
- expose a safe low-memory path that drops optional presentation assets first.

The first implementation should measure before adding a render graph or ECS
rewrite. Those are follow-up options only if traces show they are necessary.

## Stage 3 — Player experience foundation

### Controller-first semantic input

Extend `InputMap` into a device-neutral action surface supporting keyboard,
gamepad buttons, and analog navigation. Add a reusable repeat policy with
initial delay, repeat interval, edge-vs-held distinction, and focus-safe reset.

Menus consume semantic actions (`Up`, `Down`, `Confirm`, `Back`, page/shoulder
actions) rather than inspecting physical keys. Existing `MenuState` remains
compatible and gains controller navigation without game-specific debounce code.

### Recovery and accessibility UX

Add shared contracts for:

- pause-on-focus-loss policy;
- resume after suspend without a giant simulation catch-up;
- settings preview/apply/cancel;
- reset-to-defaults;
- reduced motion and bounded screen shake;
- text scale and high-contrast intent;
- device disconnect/reconnect messaging;
- haptic requests that respect the vibration setting.

Games own copy, art, and screen layout. The engine owns normalized values,
transition semantics, and input safety.

## Data flow

```text
platform events ─┐
controller state ─┼─> EngineApp ─> lifecycle/input/settings facts ─> Game
surface status ──┘                         │
                                           ├─> Time / fixed simulation
                                           ├─> Renderer / quality policy
                                           ├─> Audio / haptics
                                           └─> Diagnostics / replay probes

Game profile <─> validated SaveStore <─> native file or browser storage
```

The simulation remains the authoritative consumer of fixed-step input. The
rendering and settings layers may adapt between frames, but no adaptive policy
may mutate gameplay state or reorder commands.

## Error handling

- Lifecycle transitions are idempotent and never panic.
- Surface loss is recoverable when the backend reports it as recoverable;
  out-of-memory remains a terminal diagnostic and clean exit.
- Save replacement failures leave the last known-good primary or backup intact.
- Malformed profiles normalize to defaults and report a diagnostic.
- Missing controllers/audio/haptics are capability states, not fatal errors.
- Asset budget exhaustion rejects optional work first and increments counters.

## Verification contract

Each stage must add focused tests before implementation and finish with:

- native unit tests and strict clippy;
- WASM check/clippy for all browser-facing code;
- deterministic replay/state-hash tests proving lifecycle and quality changes
  do not alter simulation;
- save recovery tests for primary, backup, malformed, and future versions;
- controller navigation tests for edge, repeat, disconnect, and focus loss;
- browser smoke tests for pause/resume, settings, controller bridge, and
  recovery-safe rendering;
- the existing full validation script and artifact budgets.

## Explicit non-goals

- vendor console SDKs, certification, or platform store packaging;
- online accounts, cloud saves, networking, or multiplayer;
- replacing `hecs`, `wgpu`, or the current game-owned architecture;
- an in-engine editor in this pass;
- changing fixed-step simulation or state-hash schemas for presentation gains.

## Recommended delivery order

1. Lifecycle + durable persistence + profile normalization.
2. Semantic controller actions + repeat-aware menus.
3. Diagnostics, frame pacing, and adaptive presentation quality.
4. Reusable staging/asset residency improvements.
5. Game-specific UX polish and browser/native acceptance coverage.

This order makes failure recovery and player data trustworthy before optimizing
or polishing the presentation layer that depends on them.
