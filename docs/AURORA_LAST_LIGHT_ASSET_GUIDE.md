# Aurora: Last Light — Production Asset Guide

## Visual north star

Premium illustrated top-down science fiction: a lived-in industrial station
lit by purposeful machinery, not a neon grid. Materials must read before bloom.
Gameplay color is reserved: cyan/amber means Lantern utility, magenta means
Choir danger, regulatory red means Meridian authority, and lime means Verdant
terrain change.

## Camera and delivery standards

- Strict orthographic top-down view; no horizon and no three-quarter tilt.
- World scale: 64 world units per standard unit footprint.
- Source unit cell: 384×384; shipped atlas cell: 256×256 RGBA PNG.
- Structure cell: 512×512; large objectives may use 768×768.
- Shared anchor: center for hovering units; bottom-center for legged units.
- Eight directional facings only when silhouette materially changes; otherwise
  rotate one top-down frame in-engine.
- Transparent padding: at least 12% on every side; no baked cast shadows.
- Effects, shadows, selection rings, and health bars remain separate assets.

## Palette

| Role | Hex | Use |
|---|---:|---|
| Station void | `#07101A` | Deep negative space |
| Steel | `#263848` | Floors and neutral machinery |
| Lantern cyan | `#39D6D0` | Selection, power, repair |
| Lantern amber | `#F2A93B` | Work lights and resources |
| Choir magenta | `#F23B93` | Weapons, cores, warning |
| Meridian ivory | `#D7D8CC` | Armor plates |
| Verdant lime | `#8BE33A` | Living terrain and spores |
| UI bone | `#DCE9E8` | High-priority text |

Bloom should affect emissive details, never the entire silhouette.

## Tactical feedback layers

The current build keeps the playfield readable by composing feedback from
small runtime layers rather than baking it into the terrain atlas:

- **Command Surge** uses an amber selection pulse and a short-lived cyan status
  line; no new sprite sheet is required.
- **Emergency Repair** reuses the Engineer repair strip and adds a warm beam to
  the target structure or Lantern.
- **Scan Pulse** uses the existing glow texture as an expanding ring and
  reveals fog from simulation-owned coordinates.
- **Comms** reuses the six-frame portrait sheet; the panel slides in, queues
  subsequent lines, and stores a world position for the `Space` focus action.
- **Terrain bands** stay procedural: authored elevation/cover zones receive
  restrained cyan ridge or violet cover fills in-world and on the minimap. Do
  not paint these bonuses into a background texture, because combat and HUD
  need the same deterministic bounds.
- **Threat telegraphs** remain procedural as well: a magenta target pulse and
  thin attack line are layered from the enemy's live order. Keep the warning
  separate from unit art so a new Choir weapon can reuse it without another
  generated strip.

When new art is generated, keep these as separate transparent layers (unit,
effect, selection, and UI) so the browser build can budget and stream them
independently. Do not bake scan rings, radio panels, or health bars into the
terrain PNG.

## Shape language

- Lantern: open frames, repair clamps, backpacks, visible tools.
- Choir: radial symmetry, narrow blades, impossible clean joints.
- Meridian: enclosed armor, chevrons, standardized weapon housings.
- Verdant: asymmetry, branching growth, soft shells, clustered light organs.

At gameplay size, identify faction first, class second, current action third.

## Initial unit set

### Lantern Company

- **Warden** — armored squad leader; broad shoulders, cyan shield projector.
- **Engineer** — amber tool rig and two repair manipulators.
- **Surveyor** — narrow scout frame, sensor mast, teal scanning fan.
- **Mule** — resource drone with four cargo pods.
- **Bulwark** — late-game exosuit with deployable cover plates.
- **Relay Walker** — mobile power node, instantly recognizable circular mast.

### Choir of Glass

- **Needle** — fast triangular hunter.
- **Canticle** — three-armed orbital controller.
- **Bell Mine** — round armored mine with magenta apertures.
- **Silencer** — long-range beam platform with a split tuning-fork profile.
- **Assembler** — enemy builder that grows signal towers.
- **Sentinel** — boss-scale concentric machine with breakable outer vanes.

### Meridian Compact

- **Aegis Wedge** — sealed ivory shield carrier with a regulatory-red prow;
  visual source for the Bastion Accord's damage-mitigation field.
- **Charter Rig** — standardized navy fabrication crawler with numbered ivory
  hardpoints; communicates cheaper, higher-yield allied infrastructure.
- **Horizon Battery** — long ivory chassis with a red rangefinder slit and
  folding stabilizers; never reuse the Choir's radial symmetry.

### Verdant Wake

- **Bloom Heart** — black-green rooted shell with turquoise capillaries and a
  soft lime spore halo; healing effect remains a separate additive layer.
- **Briar Node** — asymmetric thorn cluster with lime warning pulses and a
  220-unit hazard ring; the core silhouette remains readable without bloom.
- **Wake Tender** — low mobile organism with three uneven fronds, used to show
  that Verdant growth can be cultivated rather than merely destroyed.

Meridian alliance sheets use four columns: idle, deploy, active field, recover.
Verdant alliance sheets use four columns: dormant, unfurl, peak emission,
settle. Both follow the same transparent 256×256 runtime-cell contract as unit
reactions, but live in faction-specific atlases to avoid coupling their palette
and animation cadence to Lantern or Choir rows.

## Required animation clips

| Class | Clips | Frames | Playback |
|---|---|---:|---:|
| Warden | idle, move, fire, hit, down | 4/6/5/3/6 | 6–12 fps |
| Engineer | idle, move, repair, build, down | 4/6/8/8/6 | 6–12 fps |
| Surveyor | idle, move, scan, mark, down | 4/6/8/4/6 | 6–12 fps |
| Choir units | idle, move, attack, break | 4/6/6/6 | 8–14 fps |
| Structures | offline, boot, idle, damaged | 1/10/4/4 | event driven |

Generate each complete strip in one pass from an approved seed. Normalize every
frame using one shared scale and anchor. Never generate individual frames as
unrelated images.

### Runtime clip contract

- One `TextureAtlas` is shared by every instance of a unit class.
- Every unit owns an `AnimationPlayer`; never share playback time across a squad.
- Stable clip IDs use lowercase verbs: `idle`, `move`, `attack`, `repair`,
  `build`, `scan`, `hit`, `down`.
- Re-selecting the active clip must not restart it. A state transition resets to
  frame zero and non-looping clips hold their last frame until the next state.
- Movement strips face screen-down in source art (world-space −Y) and rotate
  toward velocity in-engine. The runtime keeps this art-facing contract
  explicit so a generated strip cannot be rendered 180° away from its order.

### Shipped vertical-slice assets

| File | Pixel size | Grid | Runtime role |
|---|---:|---:|---|
| `last-light-units-atlas-v001.png` | 1536×1024 | 3×2 | Six unit idle silhouettes |
| `warden-move-strip-v001.png` | 2172×724 | 6×1 | Per-Warden locomotion clip (high-resolution shield silhouette) |
| `warden-attack-strip-v001.png` | 1280×256 | 5×1 | Warden shield-lance charge, fire, recoil, and settle |
| `engineer-move-strip-v001.png` | 1536×256 | 6×1 | Engineer manipulator locomotion |
| `engineer-repair-strip-v001.png` | 1536×256 | 6×1 | Tool deploy, repair beam, sparks, and recovery |
| `surveyor-move-strip-v001.png` | 1536×256 | 6×1 | Surveyor scout locomotion |
| `surveyor-scan-strip-v001.png` | 1536×256 | 6×1 | Survey mast sweep and cyan scan fan |
| `needle-attack-strip-v001.png` | 1536×256 | 6×1 | Choir Needle charge, lance, and recoil |
| `canticle-command-strip-v001.png` | 1536×256 | 6×1 | Canticle charge, command rings, and release |
| `bell-mine-arm-strip-v001.png` | 1536×256 | 6×1 | Mine apertures, warning arcs, and armed recoil |
| `unit-hit-reactions-atlas-v001.png` | 1024×1536 | 4×6 | Full-roster impact, spark, and recovery reactions |
| `unit-down-reactions-atlas-v001.png` | 1024×1536 | 4×6 | Full-roster non-gory shutdown and persistent wrecks |
| `last-light-structures-atlas-v001.png` | 1254×1254 | 2×2 | Relay, fabricator, reactor, Choir tower |
| `resource-node-atlas-v001.png` | 512×512 | 2×2 | Salvage and Flux mine nodes, idle and active frames |
| `resource-harvest-effects-v001.png` | 512×512 | 2×2 | Extraction beams, cargo lift, and depleted-node pulse |
| `terrain-detail-atlas-v001.png` | 512×512 | 2×2 | High-ground, cover, fissure, and resource-beacon overlays |
| `map-props-atlas-v001.png` | 768×512 | 3×2 | Cargo, Engineer, conduit, support, relay, and Choir route landmarks |
| `reactor-sector-v001.png` | 1672×941 | 1×1 | Authored mission floor plate |
| `portraits/lantern-command-portrait-sheet-v001.png` | 768×512 | 3×2 | Mara, Ivo, Sena, Olan, Vale, and Lumen comms portraits |

`games/last-light/src/assets.rs` is the executable catalog for this table. Its
embedded PNG-header tests fail when a file is replaced without updating the
pixel-size or grid contract, which prevents a UV crop from silently pointing
at the wrong frame. The Warden strip is intentionally retained at its
high-resolution source size; atlas UVs remain normalized and the other
runtime strips stay on the 256-pixel cell standard.

The resource-node atlas is ordered as Salvage idle, Flux idle, Salvage active,
Flux active. The renderer selects the active frame only while a Surveyor is
working the node, so the mine reads as a living gameplay object without adding
HUD clutter or unbounded world text.

The map-prop atlas is intentionally presentation-only in this pass: its six
top-down cells are authored for future map dressing and landmark placement,
while collision, fog, and interactability remain mission data. Rebuild it with
the deterministic local generator before changing the palette or cell layout:

```bash
python3 tools/asset-sources/last-light/generate_map_props_atlas.py
```

The script retains a 4× source preview in `tools/asset-sources/last-light/`
and writes the 768×512 RGBA runtime atlas under `games/last-light/assets/`.

The harvest-effects atlas is ordered as Salvage extraction, Flux extraction,
returning cargo, and depleted pulse. These are state-driven overlays rather
than replacement node art; they are drawn only while a Surveyor job is active
or while a selected/occupied node needs to communicate that it is dry.

The catalog also validates the semantic grid for each presentation role before
shipping: the environment plate is 1×1, unit and portrait sheets are 3×2,
animation strips are single-row (four or more frames), reaction atlases are
4×6, structure atlases are 2×2, resource atlases are 2×2, and resource effect
atlases are 2×2. This catches a dimensionally valid PNG that
would still crop the wrong comms portrait, reaction row, or building frame.
Run `cargo test -p last_light assets::tests` after replacing generated art.

### Player-visible state ledger

`games/last-light/src/assets.rs` also exposes
`PLAYER_VISIBLE_ART_STATES`, a small coverage ledger for every state that can
appear in the tactical view. Each entry names one of three sources:

| Source | Meaning |
|---|---|
| `Atlas` | A contiguous, validated frame range in a shipped PNG. |
| `ProceduralFallback` | A deliberate runtime composition such as the Warden shield beam or structure boot/damage overlay. |
| `PlannedAsset` | A named state that still needs dedicated authored art; it must not silently fall back to an idle frame. |

Warden attack, Engineer build, Surveyor move, and Surveyor mark have been
promoted to normalized runtime atlases (`warden-attack-strip-v001.png`,
`engineer-build-strip-v001.png`, `surveyor-move-strip-v001.png`, and
`surveyor-mark-strip-v001.png`) and are selected by their corresponding runtime
states. The executable
`NEXT_PASS_ART_CONTRACTS` table and companion
[`player-visible-art-gap-contract.yaml`](../tools/asset-sources/last-light/player-visible-art-gap-contract.yaml).
The table is intentionally empty while no planned player-visible strip
remains. The existing Engineer repair and Surveyor scan strips have different
action semantics and remain separate from the marking art.
Structure offline/boot/damaged states remain readable procedural compositions;
the Warden attack state is now a promoted five-frame authored strip. Any future
strip should first add a new planned entry, then replace it only after adding the normalized PNG to
`TextureAsset` and wiring its runtime state; the range, origin, and coverage
tests will catch a wrong grid, frame origin, or crop before the native or
browser build can ship it.

### Reaction-atlas contract

Both reaction atlases use four columns in narrative order: neutral, recoil or
failure, peak faction-colored effect, recovery or final wreck. Rows are fixed:
Warden, Engineer, Surveyor, Needle, Canticle, Bell Mine. Runtime cells are
256×256 RGBA and the complete atlas is 1024×1536.

Generated concept sheets may use unequal vertical safe zones. Normalize them
with `tools/normalize_generated_strip.py`, passing four columns, six rows, and
audited `--row-bounds` bands. The tool removes edge-connected checker/matte
backgrounds, applies one shared scale per unit row, centers every silhouette,
and preserves the existing one-row `--frames` workflow. Always inspect the
normalized alpha atlas before integration; no silhouette may cross a cell edge.

## Environment kit

- Floor: clean plate, worn plate, conduit, grate, coolant stain, breach.
- Walls: straight, inner/outer corners, door, blast door, window, damaged cap.
- Power: relay, cable straight/corner/junction, fuse box, reactor socket.
- Props: cargo stack, tool bench, pipe cluster, crane base, med locker.
- Strategic landmarks: auxiliary reactor, starforge gate, signal choir tower,
  Verdant growth heart, Vesper shuttle.
- Decals: Lantern route marks, Meridian claim stamps, Choir glyph burns.

## UI asset set

- Pointer states: default, select, move, attack, repair, harvest, forbidden.
- Selection: single ring, squad brackets, destination pulse, order path.
- Unit cards: portrait, health, shield, energy, rank, control-group number.
- Commands: move, attack, stop, hold, patrol, repair, build, ability.
- Strategic: minimap pips, fog texture, power flow, objective markers.
- Dialogue: six character portraits with neutral, urgent, and wounded variants.

UI panels use dark translucent steel with one bright state color. Keep the
center and lower-middle playfield clear outside explicit selection moments.
The production card anchors to the lower-right viewport edge and exposes no more
than three primary recipes at once. Show salvage, power, unit cap, queue depth,
and build progress with text plus color; never communicate affordability by
color alone.

The minimap anchors lower-left at a 260×138 logical-unit footprint. Friendly
contacts are cyan, visible hostiles magenta, inactive objectives gray, active
power objectives cyan, and the camera rectangle bone-white. Never draw a hidden
enemy contact. Placement previews use the structure sprite at 62–64% alpha,
cyan when valid and regulatory red when rejected, plus an understated coverage
disc beneath it.

## Audio identity

- Lantern: mechanical clicks, fabric strain, warm two-note confirmations.
- Choir: glass harmonics, reversed speech grains, precise transient attacks.
- Meridian: compressed radio, sealed hydraulics, military interval cues.
- Verdant: wet resonance, seed rattles, breath-like low frequencies.
- Music: restrained industrial pulse that adds harmonic layers as the power
  network expands; danger changes rhythm before increasing volume.

## Naming and repository layout

```text
games/last-light/assets/
  environments/<sector>/<asset>-v###.png
  factions/<faction>/<unit>/<clip>-strip-v###.png
  structures/<faction>/<structure>-v###.png
  ui/<surface>/<asset>-v###.png
  portraits/<character>/<emotion>-v###.png
  fx/<family>/<asset>-v###.png
```

Stable engine manifest keys use dots, independent of filenames:
`lantern.warden.move`, `choir.sentinel.idle`, `sector.reactor.floor`,
`ui.cursor.repair`.

## App cover art

Final app cover is stored under:

- `games/last-light/assets/cover/aurora-last-light-cover-v001.png`
- Source/pre-render asset: `tools/asset-sources/last-light/cover/aurora-last-light-cover-v001-source.png`

Current cover is a high-resolution in-game briefing frame converted to 16:9
render size and used for storefront, launcher, and repository references. Treat
it as marketing/UX-facing art rather than gameplay runtime content: do not add it
to the in-game `TextureAsset` atlas registry.

## Generated-strip normalization recipe

Generated candidates may arrive with a baked checkerboard even when the prompt
requests transparency. Never rename that file and ship it directly. Run:

```bash
python3 tools/normalize_generated_strip.py \
  --input <generated-candidate.png> \
  --output games/last-light/assets/<unit>-<clip>-strip-v###.png \
  --frames 6 --frame-size 256
```

The tool samples the corners to detect a neutral bright checker or near-black
matte, flood-removes only background connected to the slot edges, removes small
detached islands, computes one scale from the largest frame, and writes a
centered RGBA 1536×256 strip. Always inspect the final strip on a contrasting
background and confirm `(0, 255)` alpha extrema before integration.

Preserve the untouched generated candidate under
`tools/asset-sources/last-light/`. Only normalized runtime atlases belong in
`games/last-light/assets/`; this keeps production provenance available without
shipping multi-megabyte source sheets to players.

## Acceptance gates

- Silhouette readable at 64–96 screen pixels.
- Faction readable without glow or health bar.
- Alpha corners fully transparent and no chroma fringe.
- Shared anchor does not jump across the full animation strip.
- No baked text, watermark, scenery, selection ring, or shadow in unit art.
- Atlas dimensions divide exactly into declared rows and columns.
- In-engine screenshot reviewed over bright and dark floor regions.
