# Aurora Engine project guide

[Back to README](README.md)

Controls, engine capabilities, and development reference for Aurora: Last Light
and the platformer. Engine primitives live in `crates/aurora-engine`; campaign
rules and presentation remain game-owned.

## Controls

### Platformer

| Input | Action |
|--------|--------|
| **Left/Right or A/D** (select screen) | Choose a level |
| **Space / Enter** (select screen) | Start the level |
| **A/D, arrows, d-pad or stick** | Move |
| **Space / W / pad South** | Jump (hold for height) |
| **S + Space** | Drop through one-way ledges |
| **F9** | Record intents (replay capture) |
| **F10** | Replay the recording and verify the state hash |
| **Esc / P** (playing) | Pause |
| **Esc / P / Space** (paused) | Resume |
| **R / Q** (paused) | Retry / return to level select |
| **Shift / X / pad East** | Dash |
| **F3** | Toggle debug overlay |
| **Esc** (on win) | Return to level select |
| **Enter / R** (on win) | Next level / retry |

Levels hot-reload from `demos/platformer/levels/*.json` when run from that
directory — edit geometry, pickups, or tuning and save; the running game
revalidates and swaps the level instantly. Checkpoints (amber flags) re-bind
your respawn as you pass them. The level catalog contains Crystal Run, Conduit Climb, Windlift, Skyline, and
Core. The repository includes deterministic bot and replay tests; consult current
validation results before treating them as passing.

### Aurora: Last Light controls

| Input | Action |
|--------|--------|
| **Space / Enter** | Deploy from mission briefing |
| **Left click / drag** | Select one unit or a squad |
| **Shift + select** | Add units to the current selection |
| **Ctrl/Cmd + left click** | Select every visible living unit of that type |
| **Shift + Ctrl/Cmd + left click** | Add every living unit of that type |
| **Right click** | Move or attack contextually |
| **Shift + right click** | Queue a waypoint or enemy attack after the current order |
| **A, then right click** | Attack-move to a destination |
| **P, then right click** | Patrol between the current position and a waypoint |
| **U, then right click** | Follow a friendly unit |
| **Q / E / F** | Queue Warden / Engineer / Surveyor |
| **Y (single role selected)** | Activate Command Surge / Emergency Repair / Scan Pulse |
| **G (Surveyor selected)** | Route to the closest finite resource node; workers return cargo and retarget when a pocket dries up |
| **D (Fabricator selected)** | Queue a powered Supply Module (+4 capacity, 100 Salvage, 6s build) |
| **B, then left click** | Preview and deploy a powered field beacon |
| **H** | Hold selected units in position |
| **T** | Stop selected units and clear their queued paths |
| **R** | Focus the tactical camera on the next mission objective |
| **Space (tactical)** | Center the latest comms transmission; in briefing, deploy |
| **Cmd/Ctrl + 1–5** | Assign a control group |
| **1–5** | Recall and focus a control group |
| **WASD / screen edge** | Pan tactical camera |
| **Mouse wheel** | Zoom around cursor |
| **Minimap click** | Move the tactical camera |
| **Fabricator + right click** | Set the rally point for newly deployed units |
| **Esc** | Tactical pause |

### Last Light controller

| Input | Action |
|--------|--------|
| **Left stick** | Pan tactical camera |
| **Right stick** | Move the zoom-independent tactical cursor |
| **South / A / Cross** | Select at cursor; apply focused briefing or command-card action |
| **West / X / Square** | Issue contextual move, attack, harvest, patrol, follow, or rally order |
| **North / Y / Triangle** | Open contextual command focus; deploy from briefing |
| **East / B / Circle** | Cancel targeting or command focus; clear selection; resume from pause |
| **D-pad Left/Right** | Zoom tactical camera; choose a split command-card action |
| **D-pad Up/Down** | Navigate missions, briefing loadouts, and command-card rows |
| **LB/RB** | Cycle living Lantern units; change command-card pages while focused |
| **Back / Select** | Focus the next mission objective |
| **Start / Options** | Deploy from briefing or toggle tactical pause |

Controller activity automatically swaps the contextual prompts and enables a
screen-space virtual cursor. Mouse movement immediately restores pointer mode,
so a connected idle controller never competes with desktop input.

On launch, arrow keys pick a mission and `Space/Enter` opens its briefing.
The source catalog contains seven missions in progression order: Reclaim the
Reactor, A Voice in Conduit Twelve, Terms of Salvage, The Garden Below, Choir
Invisible, The Vesper Gate, and The Hollow Orbit. The later maps build on
resource, repair, terrain-control, and role-specific objectives. In A Voice in
Conduit Twelve, `K` near the extraction console with an Engineer selected and
in range wakes Lumen.

During the mission briefing, `Z`, `X`, and `C` purchase permanent Field Optics,
Reactive Plating, and Fabricator Overclock upgrades with campaign Lumen. `V`
cycles Ivo's field module and `N` cycles Sena's sensor module; both loadouts save
immediately. `M` cycles Mara's command doctrine and `O` cycles Olan's analysis
package. After establishing contact with Lumen, `L` cycles the relationship-gated
Guardian and Witness protocols. Later campaign decisions unlock `P` for Meridian
Bastion/Charter accords and `G` for Verdant Bloom/Briar covenants.

The command card is contextual: a mixed squad exposes production and squad
orders, while a single Warden, Engineer, or Surveyor exposes that role's
signature ability. Radio lines queue in the upper-right comms inbox; press
`Space` to revisit the location that prompted the latest transmission. Resource
nodes have visible worker saturation, so sending a second Surveyor is a
deliberate throughput decision rather than a hidden no-op. Visible Choir attacks
also telegraph their current target with a faction-colored line and pulse before
damage lands. Weapons resolve on authored cadences rather than every fixed tick,
with charge cues, moving bolts, impact light, hit reactions, haptics, and
persistent shutdown wrecks giving the player a readable chance to reposition or
repair. "Terms of Salvage" uses an optimized generated vault plate with ten
lane-defining cover pylons and segmented reactor bulkheads aligned to navigation.
When a selected Lantern is engaging a nearby visible contact, the camera gives
the pair a restrained screen-space framing assist; visible contacts and
actionable raid forecasts just beyond the frame receive directional edge
chevrons without leaking hidden fog contacts. The tactical HUD uses a shared
viewport-aware layout contract: telemetry and comms keep a top rhythm, while
the minimap, selected-squad card, and contextual command card share a safe
lower rail across DPR 1/2 and dense camera zoom.

Select the Engineer and command it near each of the three power relays. Active
relays generate Salvage for the Lantern fabricator; violet Flux blooms fund
advanced Surveyors. Build reinforcements, expand supply at the Fabricator, hold
the sector, defeat the Choir Canticle, and bring the auxiliary reactor online.

**Combat roles:** Idle Wardens and Surveyors automatically acquire nearby visible
enemies. Wardens are dependable front-line anchors; Surveyors trade damage for
long-range fire and harvest finite cyan salvage blooms when moved within range;
Engineers stay close to repair and operate
objectives. Choir Needles skirmish, Canticles bombard from beyond Warden range,
and Bell Mines are devastating only up close—focus them before they reach your
line.

### Campaign and production guide

- [Campaign bible](docs/AURORA_LAST_LIGHT_CAMPAIGN.md)
- [Asset production guide](docs/AURORA_LAST_LIGHT_ASSET_GUIDE.md)


## Demos

Run from the repository root unless noted:

```bash
cargo run -p playground      # particles and camera
cargo run -p platformer      # five data-driven action levels
cargo run -p aurora_run      # earlier arcade vertical slice
cargo run -p triangle_demo   # NDC triangle
cargo run -p mesh_demo       # glTF, PBR, shadows, and sky; S/K/H toggles
cargo run -p skirmish        # two-base RTS with SimpleAggroAi
```

For native platformer level hot reload, run `cargo run -p platformer` from
`demos/platformer` so the relative `levels/` watcher resolves correctly.
The mesh demo enables the engine's `3d` feature in its own Cargo manifest.

## Engine systems

These are source-described capabilities, not a release certification.

| Area | Included systems |
| --- | --- |
| Rendering | wgpu sprites, multi-texture batching, atlas packing, frustum culling, 2D lights, particles, debug shapes, and post effects including bloom, vignette, chromatic aberration, film grain, and speed lines |
| Cameras and UI | 2D pan/zoom, look-ahead/deadzone/bounds/shake, bitmap and vector text, wrapped layout, bordered and nine-slice panels, menu flow, and settings contracts |
| Simulation | Fixed steps, semantic command traces, stable hashes, cooldown ledger, deterministic FSM, scene parent-follow hierarchy, and optional `hecs` re-export via `ecs` |
| Action physics | Swept kinematic collision, solid/one-way/moving platforms, slopes, coyote time, jump buffering/cut, wall slide/jump, water buoyancy, and collision events |
| RTS primitives | Selection, formations, navigation, fog, queued orders, attack-move/patrol/follow, resource and production ledgers, supply, technology prerequisites, power graphs, placement, and minimap transforms |
| Presentation and persistence | Animations, easings/tweens, scheduling, hit-stop, parallax, audio mixing, deterministic music sequencing, gamepad input/rumble, and native/browser save contracts |
| Optional 3D | Feature-gated camera and mesh pipeline, GLB/embedded-buffer glTF loading, PBR materials, directional PCF shadows, and gradient sky |
| Native development | Shader and level file watching, file-audio playback, diagnostic and asset-load queues, and opt-in agent control |

The platformer combines these systems with per-level themes, procedural art,
checkpoints, walkers, spikes, double-jump/long-dash power-ups, a three-hit Core
boss, best-time persistence, ghost racing, and F9/F10 intent replay verification.
Its playable shell and tests use the shared headless `GameCore`.

Last Light combines shared RTS systems with specialist abilities, finite-resource
worker saturation, authored weapon cadence and telegraphs, moving bolts, repairs,
hit reactions, wrecks, mission plates, and contextual command cards. Comms and
fog-aware edge indicators help surface actionable events. Campaign progress
includes named-specialist loadouts, Lumen protocols, Meridian/Verdant decisions,
and versioned save migration.

For the public engine API, start with
[lib.rs](crates/aurora-engine/src/lib.rs) and the
[contract lab](demos/contract-lab).

## Development and validation

Prerequisites include Rust/Cargo, the `wasm32-unknown-unknown` target, Trunk,
Node/npm, Python 3, and a WebGPU-capable browser. Native builds also need the
platform libraries required by wgpu, rodio, and gilrs.

From the repository root:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
npm ci
npx playwright install chromium
./scripts/validate-local.sh
```

The gate runs:

1. Rust formatting and workspace all-target/all-feature Clippy.
2. Workspace tests, WASM workspace checking, and WASM Clippy.
3. The exact Last Light deterministic Reclaim victory replay test.
4. Last Light and platformer Trunk release builds.
5. Playwright scenarios and both delivery-budget checks.

The browser configuration serves Last Light on port 8080 and the platformer on
8082. It uses 1280×720 viewports at DPR 1 and 2, excluding the platformer agent
spec from the DPR-2 project. On macOS it selects the installed Google Chrome
executable at `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`;
installing Playwright Chromium alone does not satisfy that path.

For a focused browser pass, build both applications first:

```bash
./scripts/build-web.sh
./scripts/build-platformer-web.sh
npm run test:browser
```

The Last Light budget script caps the WASM artifact at 18 MiB and tracked PNG
source art at 20 MiB; the platformer WASM cap is 3 MiB. The budget scripts
currently use BSD/macOS `stat -f '%z'`; a GNU/Linux run needs an explicitly
reviewed compatibility approach rather than silently skipping those checks.

The repository uses local validation rather than a hosted GitHub Actions gate.
Checked-in reports describe earlier runs and are not proof that a new revision
passes. Follow the repository's contribution guidance for applicable MCP
protocol checks, deterministic traces, and native/browser screenshot review.
See [browser scenarios](playtests/browser),
[Playwright configuration](playwright.config.mjs), and
[validation script](scripts/validate-local.sh).

## MCP support for coding agents

Aurora exposes development control surfaces for launching, driving, observing,
and testing games. These are development tools, not a public runtime service.

**Native (loopback TCP, opt-in):** launch any game with `AURORA_AGENT_PORT=<port>`
and it serves newline-delimited JSON on `127.0.0.1:<port>`:

```bash
AURORA_AGENT_PORT=7837 cargo run -p platformer
# then, from any client:
#   {"id":1,"cmd":"ping"}
#   {"id":2,"cmd":"inject_key","key":"Space","down":true}
#   {"id":3,"cmd":"state"}          -> position/velocity/hash/collected/...
#   {"id":4,"cmd":"screenshot","path":"/tmp/frame.png"}
#   {"id":5,"cmd":"game","action":"load_level","args":{"index":2}}
```

**Web (JS bridge):** the WASM build automatically exposes keyboard/gamepad
injection plus `window.auroraInjectMouseMove(x, y)`,
`window.auroraInjectMouseButton(button, down)`,
`window.auroraInjectScroll(delta)`, `window.auroraGame(action, argsJson)`, and
`window.auroraState()`. A browser agent (Playwright, bookmarklet, or MCP-web
client) can therefore drive the same pointer and game-action paths as native
tooling. Browser playtests hold synthetic transitions across rendered frames so
high-DPI and low-FPS runs preserve real pressed/held/released semantics.

**Games publish state and actions** by overriding `Game::agent_state()` and
`Game::on_agent_command()`; the platformer implements `reset`, `load_level`,
and `teleport` (dev tooling), while Last Light publishes controller/combat state
and a live-unit `engage_kind` action for deterministic visual playtests.

**MCP server** (`tools/aurora-mcp`, repo-scoped, allow-listed): existing
read-only tools plus the new control tools:

| Tool | What the agent can do |
|------|-----------------------|
| `aurora_playtest_platformer` | Run the bot-verified playthrough lane |
| `aurora_validate_level` | Validate level JSON via the `level-check` bin (fail-closed) |
| `aurora_agent_control` | Launch the platformer, drive it closed-loop (start level, move player), capture a screenshot, return the transcript |
| `aurora_run_validation` | The three canonical cargo lanes (`fast`/`test`/`web`) |
| `aurora_read_source` etc. | Bounded source slices, systems map, playtest contract |

Security boundaries: the TCP server binds loopback only and is opt-in via
env; the web bridge is scoped to one page; the MCP server exposes only
allow-listed cargo invocations and bounded timeouts — never arbitrary shell.
See [tools/aurora-mcp/README.md](tools/aurora-mcp/README.md).


## License reference

See [LICENSE-MIT](LICENSE-MIT). Cargo workspace metadata declares
`MIT OR Apache-2.0`; no Apache-2.0 license file accompanies it in this tree.
This guide does not change the license or resolve that discrepancy.
