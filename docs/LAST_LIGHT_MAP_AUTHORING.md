# Last Light — Authored Map Contract

This document is the map-side companion to the asset guide. A mission is not
just a list of spawn points: it is a small tactical graph made from routes,
terrain advantages, resource risk, and readable landmarks. The same data feeds
native play, browser play, the minimap, fog, and combat.

## World envelope

- Runtime playfield: **3,600 × 2,200 world units** (`x ±1,800`, `y ±1,100`).
- Standard unit footprint: 64 world units; leave at least one footprint around
  an authored blocker when placing a spawn or objective.
- Coordinates are authored in world space, then late campaign maps may use the
  `MissionDef::expanded` transform. The transform scales every route anchor,
  blocker, terrain band, and extraction point together so the map does not
  stretch only one subsystem.
- `MissionDef::validate_layout()` is the deterministic gate for new content.
  It rejects non-finite/out-of-bounds anchors, malformed terrain, and blockers
  that contain a relay, resource, objective, or unit spawn.

## Tactical composition

Each mission should expose at least two answers to its first objective:

1. **A safe lane** — a cover pocket or a route behind a blocker where the
   Engineer or Surveyor can work.
2. **A pressure lane** — a ridge, open approach, or shorter route that rewards
   the Warden but exposes the squad to hostile fire.

Terrain bands remain data-driven `TerrainZone` rectangles. Positive elevation
is the cyan high-ground/firing position; cover (0–0.3) reduces incoming damage
without hiding the silhouette. Keep bands broad enough to matter at squad
scale, but do not stack many overlapping bands: combat consumes the first
matching zone for a target, so overlap order is meaningful and should be
intentional.

Blockers are strategic walls, not decorative borders. Use one or two clean
segments to create an L, a corridor, or an open-ended vault. Keep them away
from all authored anchors; an enemy embedded in a blocker cannot acquire a
route and a worker embedded in one cannot begin its job.

## Resource geography

The first three nodes in a mission are Salvage and the later nodes are Flux.
That ordering is part of the current runtime contract (`main.rs` assigns the
kind by index), so do not reorder the list without migrating the resource
initializer. Place nodes in a route triangle rather than a straight line:

- one node near the starting return lane (teaches the Surveyor loop),
- one node near a contested relay (creates a defend-or-greed decision), and
- one distant node that rewards a beacon, ridge, or flank.

The fourth and later nodes are Flux and can sit farther from the Fabricator;
they should be visible enough to reward a Scan Pulse, but not be free income.
Keep the finite-node lifecycle legible in the world: available → worker count
→ carrying/returning → dry.

### Contestable worker objectives

Use `MissionDef::resource_objective` when a chapter needs one resource pocket
to become a short tactical beat instead of passive income. The contract points
at a node by index (so it cannot drift from the rendered resource), names the
worker role, and may name a support role. During fixed-step simulation:

- the worker must remain inside `worker_radius` for progress to advance;
- a hostile inside `contest_radius` stalls progress and accumulates
  `contested_seconds`;
- a support unit inside `support_radius` clears that contest, letting the
  worker resume without duplicating combat or harvest rules.

The authored objective is presentation-neutral. Native/browser HUDs can read
`MissionSimulation::resource_objective_contract()` and
`resource_objective_state()` to render a compact progress card, a minimap
marker, or a radio prompt. Keep the node inside a terrain zone and include both
roles in `player_spawns`; `validate_layout()` rejects contracts that violate
either rule. Garden Below uses this pattern on its middle node: the Surveyor
works the pocket while Mara's Warden can clear the Choir's pressure.

Garden Below also opens this beat with an ambient `Time(18.0)` radio line:
Sena warns that the middle cache is contested and asks Mara to clear the roots
before she commits the Surveyor. It is deliberately authored before the first
relay-gated line, so the ordered radio cursor can always deliver the warning
even if the player never staffs the node. The later
`ResourceObjectiveCompleted` line remains the earned completion handoff rather
than a prerequisite for ordinary campaign dialogue. Keep the same ordering
rule whenever a future mission adds optional resource telemetry.

### Terrain-control objectives

Use `MissionDef::terrain_control_objective` when a role must actually claim a
terrain advantage instead of merely reaching a landmark. The contract names a
unit, target radius, minimum elevation/cover, and hold duration. During fixed-
step simulation, the runtime should resolve the strongest `TerrainZone` at the
target and feed that readout into `TerrainControlState::advance()`:

- the correct role outside the target radius is `Waiting`;
- a role on the relay's covered apron but below the required ridge is
  `WrongTerrain`;
- an enemy in the control radius is `Contested` and stalls progress while
  preserving seconds already earned;
- clearing the enemy lets the role resume and eventually emit `Completed`.

This gives the map two readable answers: a lower safe lane can buy time, but
only the pressure lane's ridge completes the authored control beat. Terms of
Salvage uses `TerrainControlObjective::high_ground_hold()` on its first relay;
the narrow positive-elevation band intentionally overlaps the lower covered
apron so the player must decide when Mara can leave the safe side and claim the
firing angle. Keep the objective target inside a resolved matching zone—layout
validation rejects missing or mismatched terrain—and keep the hold short enough
that a raid creates pressure without turning it into a second victory
condition.

The third Garden Salvage cache is a separate eastern flank choice. Its small
positive-elevation zone uses lighter cover than the reactor apron, so a Warden
can take a firing angle around the root wall without turning the route into a
safe corridor. The objective contract stays on the middle cache: the Surveyor
must hold the worker radius, Mara can clear the larger contest radius, and the
fixed eight-second sequence is replayable even when the flank is chosen instead.

The following `Time(24.0)` Ivo line makes the economy loop actionable in the
same pass: it recommends spending the cache's first return on a Warden through
the Fabricator before holding the roots. This is an authored advisory rather
than a queue-depth trigger, so it remains useful when the player is still
learning the build menu and cannot stall the radio cursor. If a future runtime
adds a stateful build trigger, keep an unconditional fallback before the first
relay/resource gate.

`MissionDef::validate_layout()` also requires at least three nodes and keeps
resource anchors at least 128 world units apart (two standard unit footprints).
This is deliberately smaller than the authored route spacing: it catches
accidental duplicate or stacked nodes without dictating the safe/contested/
distant choices that make a map interesting.

## Current authored chapters

| Mission | Map promise | Terrain / route signature | Resource tension |
|---|---|---|---|
| Reclaim the Reactor | Teach the complete loop in an open sector | Central cyan ridge, covered middle salvage pocket, eastern high-ground perch | Nearby Salvage teaches return; distant Flux asks for a protected route |
| A Voice in Conduit Twelve | Escort discipline through a maintenance spine | Long horizontal lanes, vertical gates, high deck, covered middle cache and extraction | The Surveyor can work the contested cache under light cover, but must still protect Sena's route |
| Terms of Salvage | Make the vault feel claimed rather than crossed | Expanded high-ground vault, covered safe-start cache, a narrow first-relay control ridge, and L-shaped support blocker | The first Salvage node teaches a defendable economy; later Flux caches stay exposed and optional; Mara must hold the ridge to secure the relay beat |
| The Garden Below | Protect the escort while choosing infrastructure greed | Expanded root walls, high central ridge, light-cover eastern flank perch, covered relay/extraction pockets | Six nodes fund optional relays; the middle cache is a timed Surveyor/Warden hold while the eastern cache offers a greedier flank |
| Choir Invisible | Make sensor coverage a contested offensive resource | 1.16× expanded blackout chambers, a covered low maintenance lane, and a high-ground eastern sensor deck over the final relay | Six nodes place Salvage at the start and contested center, then Flux on three exposed flanks; the Surveyor/Warden cache contract and Warden deck hold compete for the same midgame attention |
| The Vesper Gate | Turn the campaign branch into a coordinated gate assault | 1.08× expanded twin gate walls, a central cross-gate, covered worker lane, exposed reactor apron, and an eastern high-ground ridge | Six nodes teach a safe opening cache, a contested middle mark, and optional northern Flux; Surveyor cache security, Engineer repair, and Warden ridge control overlap in the same midgame window |
| The Hollow Orbit | Turn the gate escape into a three-route orbital-ring assault | 1.18× expanded ring, twin bulkheads, a central coolant break, mined service lane, and eastern high-ground ridge | Seven nodes place a safe opening pocket beside the Fabricator, a Canticle-exposed dead-orbit cache, an Engineer coolant target, and optional north/south Flux arcs |

### Mission 6: Choir Invisible

`choir_invisible()` is the first authored branch beat after Garden Below. Its
three-relay victory still uses the shared “restore power, defeat the Canticle”
contract, but the route geometry changes the order in which those jobs feel
safe:

- The opening Salvage node sits in a covered pocket beside the Fabricator,
  giving the Surveyor a dependable return loop.
- Node index `1` is the contested middle cache. A Surveyor must stay inside its
  worker radius while a Warden clears the larger support/contest radius; the
  objective is finite and deterministic, so radio completion cannot depend on
  frame timing or a particular render path.
- The eastern relay sits inside a positive-elevation sensor deck. The Warden's
  terrain-control objective resolves that deck rather than the lower apron, so
  taking the safe lane alone cannot complete the hold.
- Four axis-aligned blackout blockers create alternate openings: the lower
  maintenance route protects worker traffic, while the upper deck is a shorter
  pressure route with less cover. The final three resource nodes are Flux and
  remain optional flank income instead of free early saturation.

All coordinates, terrain bands, blockers, and objective radii are expanded by
the same `1.16` transform before validation. Keep that transform intact when
editing the map; changing one coordinate family by hand would desynchronize
the minimap, navigation, combat cover, or objective telemetry.

### Mission 7: The Vesper Gate

`vesper_gate()` is the next branch beat after Choir Invisible. It keeps the
three-relay/Canticle victory contract but makes the gate itself a coordinated
specialist puzzle:

- The opening Salvage node is covered beside the Fabricator, while node index
  `1` is the false-exit cache that requires a Surveyor mark and Warden support.
- The auxiliary reactor is an exposed repair target for Ivo. The Engineer
  objective is separate from the cache and ridge timers, so a player cannot
  complete the chapter by parking one role in a generic objective zone.
- The eastern relay resolves to positive-elevation terrain. Mara must hold the
  ridge while the second gate wall blocks a direct center-line attack.
- The northern Flux caches are optional and exposed; the southern bulkhead
  keeps early income from becoming a free saturation route.

The mission expands authored coordinates by `1.08` before layout validation.

### Mission 8: The Hollow Orbit

`hollow_orbit()` is the next branch beat after Vesper Gate. It keeps the
three-relay/Canticle victory contract while giving each Lantern role a clear
counterplay job from the opening radio prefix:

- The covered western pocket supplies the first Surveyor return loop; the
  central dead-orbit cache is intentionally exposed to Canticle fire and needs
  Warden support rather than a blind worker rush.
- The Engineer's coolant-core repair target sits behind a mined service lane.
  Bell Mines punish a direct crossing, while Needles try to pull the Warden
  off the eastern ridge.
- The eastern ridge is a separate terrain-control objective, so clearing the
  cache or reactor never silently completes the hold. Optional Flux arcs reward
  a later Surveyor detour after the first relay is safe.

The map expands authored coordinates by `1.18` before layout validation.

## Asset relationship

The environment plate is a mood and material layer, not a collision map. Keep
collision blockers, elevation, cover, fog, and resource state in authored data
so they remain deterministic and testable. The procedural overlays (terrain
edges, minimap pips, scan rings, threat telegraphs) should stay separate from
the PNG; this lets a new map reuse the existing atlas without baking gameplay
state into art.

When a new environment or landmark asset is approved, add it to the executable
texture catalog and its dimension/grid test before wiring it to a mission. Use
the existing transparent atlas palette and normalize complete animation strips
as one pass; do not create one-off frame sizes for a single map.

## Review checklist

- [ ] `cargo test -p last_light missions::tests` passes.
- [ ] `MissionDef::validate_layout()` returns `Ok(())` for every mission.
- [ ] Every blocker has an alternate route around an open edge.
- [ ] Every objective has one safe lane and one pressure lane.
- [ ] A terrain-control objective resolves to the intended elevation/cover band,
      not merely to an overlapping decorative zone.
- [ ] No resource node or spawn sits inside a blocker or outside the playfield.
- [ ] The minimap still shows terrain and resources without adding persistent
      labels over the center of play.
- [ ] Native and browser screenshots show faction silhouettes against both the
      environment plate and the procedural terrain overlays.
