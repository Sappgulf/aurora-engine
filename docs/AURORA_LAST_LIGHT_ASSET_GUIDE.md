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
- Movement strips face north in source art and rotate toward velocity in-engine.

### Shipped vertical-slice assets

| File | Grid | Runtime role |
|---|---:|---|
| `last-light-units-atlas-v001.png` | 3×2 | Six unit idle silhouettes |
| `warden-move-strip-v001.png` | 6×1 | Per-Warden locomotion clip |
| `engineer-move-strip-v001.png` | 6×1 | Engineer manipulator locomotion |
| `surveyor-scan-strip-v001.png` | 6×1 | Survey mast sweep and cyan scan fan |
| `needle-attack-strip-v001.png` | 6×1 | Choir Needle charge, lance, and recoil |
| `canticle-command-strip-v001.png` | 6×1 | Canticle charge, command rings, and release |
| `bell-mine-arm-strip-v001.png` | 6×1 | Mine apertures, warning arcs, and armed recoil |
| `unit-hit-reactions-atlas-v001.png` | 4×6 | Full-roster impact, spark, and recovery reactions |
| `unit-down-reactions-atlas-v001.png` | 4×6 | Full-roster non-gory shutdown and persistent wrecks |
| `last-light-structures-atlas-v001.png` | 2×2 | Relay, fabricator, reactor, Choir tower |
| `reactor-sector-v001.png` | single | 2600×1460 authored mission floor |

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
examples/last_light/assets/
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

## Generated-strip normalization recipe

Generated candidates may arrive with a baked checkerboard even when the prompt
requests transparency. Never rename that file and ship it directly. Run:

```bash
python3 tools/normalize_generated_strip.py \
  --input <generated-candidate.png> \
  --output examples/last_light/assets/<unit>-<clip>-strip-v###.png \
  --frames 6 --frame-size 256
```

The tool samples the corners to detect a neutral bright checker or near-black
matte, flood-removes only background connected to the slot edges, removes small
detached islands, computes one scale from the largest frame, and writes a
centered RGBA 1536×256 strip. Always inspect the final strip on a contrasting
background and confirm `(0, 255)` alpha extrema before integration.

## Acceptance gates

- Silhouette readable at 64–96 screen pixels.
- Faction readable without glow or health bar.
- Alpha corners fully transparent and no chroma fringe.
- Shared anchor does not jump across the full animation strip.
- No baked text, watermark, scenery, selection ring, or shadow in unit art.
- Atlas dimensions divide exactly into declared rows and columns.
- In-engine screenshot reviewed over bright and dark floor regions.
