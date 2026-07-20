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

`FOUNDRY-007`: Contextual specialist command cards, deterministic role
abilities, finite worker saturation, and a queued comms inbox with world-space
transmission focus. The shared engine `CooldownBook` keeps ability timing
fixed-step and hashable across native and WASM.

`FOUNDRY-008`: Expanded Terms of Salvage with a resolved high-ground ridge over
the covered relay apron, deterministic terrain-control progression/contest
state, runtime Warden hold feedback, and procedural structure state overlays
for booting, offline, and damaged buildings. The compact Fabricator module and
resource gate copy remains readable at the reference native/browser viewport.
Evidence: 124 Last Light tests, 4 browser checks at DPR 1/2, and the fresh
native package playtested through movement, Warden ability, Surveyor harvest,
comms, and Fabricator module progress.

## Next iteration

`FOUNDRY-009`: Turn terrain-control completion into a broader campaign branch
beat, add a second authored map with a distinct resource/defense geometry, and
extend the native/browser playtest contract to exercise combat telegraphs and
terrain contest transitions. The first content slice is now in place: Mission
6, `CHOIR INVISIBLE`, adds a three-relay blackout map, a contested worker cache,
an eastern high-ground sensor deck, named specialist spawns, and authored radio
beats; the engine also exposes FIFO Shift+right-click attack queues and the
asset ledger explicitly tracks atlas/procedural/planned player-visible states.
Browser combat/terrain coverage is now verified at DPR 1 and 2; Engineer build,
Surveyor move, and Surveyor mark are normalized authored atlases selected by
their runtime animation states. The Vesper Gate branch is now authored as the
next three-role beat: Surveyor cache security, Engineer reactor repair, and
Warden ridge control share a widened twin-gate map with distinct worker and
defense routes. FOUNDRY-010 extends that branch through The Hollow Orbit with
three-route orbital geometry, a mined Engineer lane, Canticle-exposed Surveyor
cache, and a separate Warden ridge hold; it also promotes Warden attack art and
the shared engine Stop command. Native and browser smoke now cover both late
missions. The next slice can focus on deeper faction counterplay, production
depth, and additional authored landmark art.
