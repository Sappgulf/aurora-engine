# Current Epic: Reclaim the Reactor Truth Machine

Build a deterministic headless scenario runner plus browser screenshot
playtest for Reclaim the Reactor covering selection, movement, relay
restoration, production, combat, and victory.

## Epic acceptance

- A renderer-free Last Light mission simulation accepts semantic commands at fixed ticks.
- Two clean runs of the canonical scenario produce the same final state hash.
- The run emits a bounded event log and `.aurora-trace` payload.
- Native and browser scenarios exercise the same player intent.
- Browser screenshots cover 1280×720 at DPR 1 and 2 with no HUD overlap.
- CI runs the headless scenario and builds the actual Trunk application.
- MCP exposes only allow-listed scenario IDs and bounded reports.

## Completed iterations

`FOUNDRY-001`: Foundry bootstrap and generic semantic trace/state-hash
contract. Evidence is recorded in `reports/latest.json`.

`FOUNDRY-002`: Renderer-free Last Light roster, selection, navigation, and
movement simulation with matching Reclaim trace hashes.

`FOUNDRY-003`: Simulation-owned relay restoration, power, resources, and
production with a bounded ordered event log and persisted 900-tick Reclaim
truth trace.

`FOUNDRY-004`: Simulation-owned combat, unit destruction, Canticle
reinforcement, defeat, and victory; the 3600-tick Reclaim trace now completes
the mission and its bounded report is available through an allow-listed MCP
scenario id.

`FOUNDRY-005`: Automated Chromium captures cover mission select, tactical
pause, and production at 1280×720 under DPR 1 and 2 with safe-zone, backing
store, visible-HUD, and console assertions. CI runs the canonical scenario,
actual Trunk build, browser lane, and uploads screenshot evidence.

`FOUNDRY-006`: Combat snapshot and attack work now reuse simulation-owned
32-entry buffers. The canonical 3600-tick victory trace asserts both buffer
capacities remain unchanged while preserving matching final hashes.

## Next iteration

`FOUNDRY-007`: Remove path-following and HUD command-card transient frame
allocations, add capacity/budget assertions for both, and preserve native and
browser interaction behavior.
