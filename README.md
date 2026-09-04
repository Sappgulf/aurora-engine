# Aurora Engine

**Fast Rust game engine · beautiful wgpu graphics · desktop + browser**

> Status: **v0.4 / engine build-out** — glTF + shadow-mapped 3D path, shader
> hot reload, vector fonts, file audio, haptics, and a fully built-out
> platformer pack (three CI-playtested levels, checkpoints, replay
> verification) alongside the Last Light point-and-click campaign.

## Play now

```bash
cd ~/dev/aurora-engine
cargo run -p last_light
cargo run -p platformer
```

### Platformer pack (physics + level showcase)

| Input | Action |
|--------|--------|
| **Left/Right or A/D** (select screen) | Choose a level |
| **Space / Enter** (select screen) | Start the level |
| **A/D, arrows, d-pad or stick** | Move |
| **Space / W / pad South** | Jump (hold for height) |
| **S + Space** | Drop through one-way ledges |
| **F9** | Record intents (replay capture) |
| **F10** | Replay the recording and verify the state hash |
| **Esc** | Back to level select |
| **Enter / R** (on win) | Next level / retry |

Levels hot-reload from `demos/platformer/levels/*.json` when run from that
directory — edit geometry, pickups, or tuning and save; the running game
revalidates and swaps the level instantly. Checkpoints (amber flags) re-bind
your respawn as you pass them. All three shipped levels are proven beatable
by the deterministic playthrough bot on every test run.

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

On launch, `Up/Down` (or `Left/Right`) pick a mission from the mission-select
screen and `Space/Enter` deploy — mission 3, "A Voice in Conduit Twelve,"
unlocks after completing "Reclaim the Reactor." Mission 4, "Terms of Salvage,"
opens the Meridian branch; mission 5, "The Garden Below," opens the Verdant
branch; mission 6, "Choir Invisible," turns blackout terrain into a resource
and sensor puzzle; mission 7, "The Vesper Gate," combines Surveyor cache
security, Engineer reactor repair, and Warden ridge control into a coordinated
gate assault; mission 8, "The Hollow Orbit," adds a mined coolant lane and a
Canticle-exposed dead-orbit cache. During mission 3, `K` near
the extraction console (while an Engineer is selected and in range) wakes
Lumen.

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

## Other demos

```bash
cargo run -p playground      # free-roam particles / camera
cargo run -p platformer      # 3 data-driven levels: checkpoints, movers, replay verify; bot-tested
cargo run -p aurora_run      # earlier arcade vertical slice
cargo run -p triangle_demo   # M0 NDC triangle
cargo run -p mesh_demo       # 3D: glTF + PBR + shadow maps + sky; S/K/H toggles
cargo run -p skirmish        # free-play two-base RTS skirmish vs. the engine's SimpleAggroAi
```

## Features

| System | Notes |
|--------|--------|
| wgpu sprites + multi-texture batching | ✅ |
| Camera2D pan/zoom | ✅ |
| Input (keys/mouse/scroll) | ✅ |
| Fixed timestep | ✅ |
| Semantic command traces + stable state hashing | ✅ foundation |
| Post-FX (bloom, vignette, chromatic) | ✅ |
| Texture atlases + `Animation` | ✅ |
| Audio beeps (rodio / Web Audio) | ✅ |
| AABB collision | ✅ |
| Particles | ✅ |
| WASM / Trunk scaffold | ✅ |
| Feature-gated 3D camera conventions | ✅ foundation |
| Menu flow + bitmap text UI | ✅ |
| Save/settings + audio mixer contracts | ✅ |
| Tile collisions/triggers + camera rig | ✅ |
| Diagnostics + asset loading queue | ✅ foundation |
| Deterministic cooldown ledger for gameplay abilities | ✅ |
| Feature-gated 3D mesh pipeline (depth-tested, single-light PBR) | ✅ core |
| RTS selection, formations, orders, navigation, fog | ✅ |
| RTS economy, two-resource production, supply, tech prerequisites, power graphs, control groups | ✅ |
| Attack-move, patrol, follow, queued waypoints | ✅ |
| Contextual specialist command cards, signature abilities, and comms inbox | ✅ |
| Combat damage classes, armor, elevation, and cover zones | ✅ |
| Structure lifecycle (health/build/power) and Engineer repairs | ✅ |
| Economy-aware Choir raids and data-driven campaign triggers | ✅ |
| Placement validation, tactical minimap, per-unit clip players | ✅ |
| Generated Engineer, Surveyor, and Choir action strips | ✅ |
| Full-roster hit reactions and persistent shutdown wrecks | ✅ |
| Authored weapon cadence, windups, moving bolts, impact lighting, and rate-limited combat audio | ✅ |
| Mission-specific generated environment plates + segmented navigation-matched bulkheads | ✅ |
| Combat framing assist + fog-safe off-screen threat chevrons | ✅ |
| Shared viewport HUD anchors, safe-area spacing, and DPR-stable lower command rail | ✅ |
| Last Light deterministic ambient mission bed using the engine sequencer | ✅ |
| Four persistent named-specialist doctrine/loadout pairs | ✅ |
| Relationship-gated Lumen Guardian/Witness protocols | ✅ |
| Meridian and Verdant alliance doctrine foundations | ✅ |
| Versioned campaign progress with v1 save migration | ✅ |
| Data-driven mission definitions + in-game mission select | ✅ 7-mission campaign |
| Generic `SimpleAggroAi` (target scoring, retreat, focus-fire, `NavGrid` pathing) | ✅ |
| Skirmish free-play mode (reuses RTS core, no campaign/art dependency) | ✅ |
| Swept kinematic physics (`physics2d`: solid/one-way/moving colliders, tunnel-proof) | ✅ |
| Character controller (coyote time, jump buffer/cut, apex hang, wall slide/jump) | ✅ |
| Camera look-ahead + existing rig deadzone/bounds/shake for action games | ✅ |
| Platformer demo exercising the full physics pack | ✅ |
| Juice kit: easings, tweens (tagged runner), scheduler, hit-stop, parallax | ✅ |
| Generic deterministic FSM helper for actors/UI flow | ✅ |
| Gamepad input (gilrs native + Web Gamepad, standard mapping, stick dead zone) | ✅ |
| Data-driven levels: JSON defs, fail-closed validation, per-level tuning | ✅ |
| CI playthrough bot proves the shipped platformer level beatable (6/6) | ✅ |
| Scene parent-follow hierarchy with cycle guards | ✅ |
| Last Light point-and-click campaign missions | ✅ 7-mission vertical slice |
| glTF 2.0 loader (GLB + embedded buffers, node transforms, PBR materials) | ✅ `3d` feature |
| Directional-light shadow maps (PCF, depth pass) + gradient sky w/ sun disk | ✅ `3d` feature |
| Shader hot reload (WGSL overrides from disk, validation-safe) | ✅ native |
| Vector font text rendering (ab_glyph atlas, layout, wrap, kerning) | ✅ |
| File audio playback (looping music + one-shot SFX via rodio) | ✅ native |
| Gamepad rumble/haptics (gilrs force feedback + Web `vibrationActuator`) | ✅ |
| Platformer level select + 3 shipped levels + checkpoint respawns | ✅ |
| Intent recorder + replay state-hash verification (tests + F9/F10) | ✅ |
| Level JSON hot-reload watch mode (`devtools::FileWatcher`) | ✅ native |
| Film grain post effect (`post_fx.film_grain`) | ✅ |
| Optional `hecs` ECS re-export (`ecs` feature) | ✅ |
| Platformer WASM/Trunk scaffold (web build) | ✅ |
| Agent control plane: loopback TCP server + web JS bridge (`agent` module) | ✅ |
| Game hooks: `agent_state()` + `on_agent_command()` with platformer actions | ✅ |
| MCP control tools: playtest lane, level validation, closed-loop agent drive | ✅ |
| Browser agent spec: Playwright drives the WASM build via `window.aurora*` | ✅ |
| Browser mouse/scroll injection, game-specific actions, and combat engagement state | ✅ |
| Platformer presentation: 2D light pass (lantern/crystals/checkpoints), aurora bands, edge-lit tiles, trails, confetti, death flash | ✅ |
| Persistent best times per level (`SaveStore`, native + localStorage) | ✅ |
| Procedural asset pack: animated character (run/jump/fall), flag, beveled tiles, ferry plating, clouds | ✅ |
| Feel physics: head-bonk corner correction, ledge-lip step-up, skid turnarounds, wall-jump buffering | ✅ |
| UI kit: bordered panels, HUD chips, level-select cards, win overlay with NEW BEST badge | ✅ |
| Pause menu (P/Esc: resume, retry, levels) + F3 debug overlay (fps, state, hash) | ✅ |
| Parameterized MCP scenario tool: agents author and run their own bounded plans | ✅ |
| Bot runs replay bit-identically on every shipped level + 600-mutation validator fuzz | ✅ |
| Walkers (stomp to kill) + spike hazards with respawn grace | ✅ |
| Ghost racing: your best run replays as a translucent rival | ✅ |
| Procedural ambient music (engine note sequencer, deterministic) | ✅ |
| 9-slice rounded panels + bitmap text shadows | ✅ |
| Sprite frustum culling + runtime atlas packing | ✅ |
| Speed-line post pass keyed off dash state | ✅ |
| Debug shape renderer (AABBs, rays, grid) | ✅ |
| Collision event stream (`physics_step_events`) | ✅ |
| Water zones + buoyancy physics | ✅ |
| Rate-based particle emitters (level-driven ambience) | ✅ |
| Level themes: per-level palettes (sky, terrain, accent) | ✅ |
| Power-ups: double jump + long dash | ✅ |
| CORE boss arena: 3-stomp fight, bot-verified | ✅ |
| Agent level-authoring loop (level-check --solve + MCP `aurora_level_author`) | ✅ |
| MCP evidence gallery (12 tools total) | ✅ |

## Library sketch

```rust
use aurora_engine::{run, Aabb, FrameCtx, Game, Sprite};

impl Game for MyGame {
    fn on_fixed_update(&mut self, ctx: &mut FrameCtx<'_>) {
        if self.player.intersects(self.coin) {
            ctx.audio.collect();
            self.score += 1;
        }
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        ctx.renderer.post_fx.bloom_intensity = 1.0;
        ctx.renderer.draw_sprite(self.tex, Sprite::new(...));
    }
}
```

## Browser

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cd games/last-light && trunk serve
```

Automated browser evidence uses the actual Trunk output at 1280×720 under DPR
1 and 2:

```bash
npm ci
npx playwright install chromium
./scripts/build-web.sh
npm run test:browser
```

See [playtests/README.md](playtests/README.md) for checkpoints and assertions.

## Local release gate

Aurora deliberately uses a reproducible local release gate instead of hosted
GitHub Actions. Run the native, feature-gated, deterministic, and WASM checks
with:

```bash
./scripts/validate-local.sh
```

Then build the browser deliverable and run its screenshot-based gameplay checks:

```bash
./scripts/build-web.sh
npm run test:browser
```

`./scripts/check-web-budget.sh` enforces the current 18 MiB WASM and 20 MiB
source-art budgets after the web build. The limits are deliberately visible: an
asset addition that exceeds either one needs a delivery decision, not a silent
download-size regression.

## MCP support for coding agents

Aurora gives agents a **closed-loop control plane** — the same surface a
human player has — so an agent can launch, play, observe, and verify the
engine end-to-end, not just read its source.

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

## License

MIT OR Apache-2.0 · Repo: https://github.com/Sappgulf/aurora-engine
