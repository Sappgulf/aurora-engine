# Foundry Learnings

## 2026-07-19 — bootstrap audit

- The repository is young (33 commits in one day) and ownership is concentrated in one contributor; executable boundaries matter more than inferred conventions.
- `crates/aurora-engine/src/renderer.rs` is both a history hotspot and a bug magnet. Allocation work should follow the truth-machine epic so performance changes gain repeatable evidence.
- Last Light already has game-owned mission, campaign, save, asset, and unit modules, but `main.rs` still combines simulation with renderer and input callbacks.
- Aurora's existing timestamped native input harness is presentation-oriented. The deterministic layer should record semantic commands at integer ticks instead of device events at wall-clock milliseconds.
- Existing untracked playtest screenshots and `.playwright-mcp/` belong to the user and must remain untouched.

## 2026-07-19 — FOUNDRY-001

- Semantic traces can stay game-agnostic with an action string and structured payload; command interpretation remains behind the game's `DeterministicSimulation` implementation.
- Serialized state hashing is safe only for deterministically ordered snapshots. Simulations with hash maps should sort entries or use `StableStateHasher` directly.
- Strict all-target Clippy exposed test-only contract code leaking into the normal demo binary; `#[cfg(test)]` preserved the proof without shipping dead code.
- The local Python environment lacked MCP dependencies. The declared requirements passed from an isolated `/tmp/aurora-mcp-venv`; this was an environment prerequisite, not a protocol defect.

## 2026-07-19 — FOUNDRY-002

- Roster creation, spawn modifiers, selection, movement formation, obstacle routing, and path advancement form a coherent renderer-free seam.
- Campaign upgrades enter the simulation as `SpawnModifiers`; the simulation does not import save or campaign-screen state.
- Animation players remain presentation-owned and are rebuilt from simulation unit IDs after mission construction.
- The live game and canonical trace now use the same `MissionSimulation`; the 180-tick Reclaim selection-and-move trace matched across two clean runs.
- A clean Safari reload confirmed right-click movement, three-unit survival, HUD integrity, and minimap response after the extraction.

## 2026-07-19 — FOUNDRY-003

- Relay restoration, power activation, passive relay income, production spending, queue timing, and reinforcement spawning now share one renderer-free owner.
- Presentation consumes bounded simulation events for relay audio, animation-player creation, and deployment status instead of duplicating gameplay transitions.
- The checked-in 900-tick Reclaim trace restores relay one and produces exactly one additional Warden with matching hashes across two clean runs.
- Safari confirmed the same production command spends salvage, updates the queue HUD, completes, and returns the queue to ready without HUD overlap.
- The retained event history and presentation event queue are both capped at 256 entries; headless traces cannot grow either queue without bound.

## 2026-07-19 — FOUNDRY-004

- Attack resolution, health changes, unit destruction, the Canticle reinforcement phase, and mission outcomes now belong to `MissionSimulation`; presentation consumes semantic events for flashes, wrecks, audio, and overlays.
- Game-owned campaign doctrines cross the boundary as damage and damage-taken scales, keeping save and faction concepts out of the renderer-free simulation.
- The canonical Reclaim trace now restores all three relays, produces one Warden, attacks the Canticle, triggers its reinforcement phase, and reaches victory at tick 3600 with matching hashes across two runs.
- Combat retains the pre-existing per-tick snapshot/work-vector allocation pattern moved from `main.rs`; FOUNDRY-004 introduces no new steady-state allocation class, and optimization can now use deterministic evidence.
- MCP exposes only `last_light.reclaim.relay_production`, returning checked-in trace/report metadata with a 64-command cap and no path or executable input.
- Safari showed live movement into combat, synchronized attack/hit effects and health changes, a readable HUD, and an updated minimap on the latest WASM build.

## 2026-07-19 — FOUNDRY-005

- A browser screenshot checkpoint needs both fixed input cadence and content assertions; dimension-only checks initially accepted a DPR-2 frame that remained paused because SwiftShader had not rendered between key events.
- Mission select, tactical pause, and active production now capture at 1280×720 CSS pixels with 1× and 2× backing stores, producing six CI artifacts per run.
- HUD geometry is an explicit game-owned contract: objective, pause, minimap, and command-card regions cannot overlap one another or the protected central playfield.
- PNG pixel inspection verifies that the production command card is actually visible, preventing a pause overlay or blank renderer from satisfying geometry alone.
- Playwright 1.61.1 and pngjs 7.0.0 install with zero audited vulnerabilities; generated screenshots and test traces remain ignored evidence, not source assets.

## 2026-07-19 — FOUNDRY-006

- The combat roster is bounded enough that a 32-entry linear snapshot is simpler and faster than rebuilding a `HashMap` every fixed tick.
- Simulation-owned snapshot and attack vectors preserve capacity across the full 3600-tick Reclaim victory trace, including one produced Warden and two Canticle reinforcements.
- The existing command order, damage order, event order, final victory, and matching state hashes remained unchanged after removing the temporary collections.
- Preallocating the pending presentation-event queue also removes its first-combat growth without changing the existing 256-event bound.
