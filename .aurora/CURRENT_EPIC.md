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

## Next iteration

`FOUNDRY-004`: Move combat, defeat, and victory resolution into
`MissionSimulation`, extend the canonical trace through mission completion,
and expose its bounded report through the allow-listed MCP scenario lane.
