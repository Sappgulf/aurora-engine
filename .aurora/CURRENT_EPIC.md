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

## Completed iteration

`FOUNDRY-001`: Foundry bootstrap and generic semantic trace/state-hash
contract. Evidence is recorded in `reports/latest.json`.

## Next iteration

`FOUNDRY-002`: Extract a renderer-free `MissionSimulation` seam from Last Light
and replay the first selection-and-move trace through it twice.
