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

