# Aurora Engine

**Fast Rust game engine · beautiful wgpu graphics · desktop + browser**

> Status: **v0.4 / Milestone 4 campaign** — point-and-click command simulation, six authored missions, native + browser.

## Play now

```bash
cd ~/dev/aurora-engine
cargo run -p last_light
```

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
damage lands, giving the player a readable chance to reposition or repair.

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
cargo run -p aurora_run      # earlier arcade vertical slice
cargo run -p triangle_demo   # M0 NDC triangle
cargo run -p mesh_demo       # M4 core 3D pipeline: lit, depth-tested cube + sphere
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
| Four persistent named-specialist doctrine/loadout pairs | ✅ |
| Relationship-gated Lumen Guardian/Witness protocols | ✅ |
| Meridian and Verdant alliance doctrine foundations | ✅ |
| Versioned campaign progress with v1 save migration | ✅ |
| Data-driven mission definitions + in-game mission select | ✅ 7-mission campaign |
| Generic `SimpleAggroAi` (target scoring, retreat, focus-fire, `NavGrid` pathing) | ✅ |
| Skirmish free-play mode (reuses RTS core, no campaign/art dependency) | ✅ |
| Last Light point-and-click campaign missions | ✅ 7-mission vertical slice |

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

`./scripts/check-web-budget.sh` enforces the current 18 MiB WASM and 12 MiB
source-art budgets after the web build. The limits are deliberately visible: an
asset addition that exceeds either one needs a delivery decision, not a silent
download-size regression.

## MCP support for coding agents

Aurora includes a local, repository-scoped MCP server for model-assisted engine
work. It provides a systems map, bounded source slices, the Last Light playtest
contract, and explicitly selected Cargo validation lanes—never arbitrary shell
commands or arbitrary filesystem reads. See [tools/aurora-mcp/README.md](tools/aurora-mcp/README.md) for
installation, client configuration, security boundaries, and the stdio protocol
smoke test.

## License

MIT OR Apache-2.0 · Repo: https://github.com/Sappgulf/aurora-engine
