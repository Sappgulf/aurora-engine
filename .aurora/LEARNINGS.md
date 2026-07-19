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
