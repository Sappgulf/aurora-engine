# Aurora Engine

**Fast Rust game engine · beautiful wgpu graphics · desktop + browser**

> Status: **v0.3 / Milestone 3 RTS** — point-and-click command simulation, authored campaign, native + browser.

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
| **Right click** | Move or attack contextually |
| **Q / E / F** | Queue Warden / Engineer / Surveyor |
| **B, then left click** | Preview and deploy a powered field beacon |
| **H** | Hold selected units in position |
| **Cmd/Ctrl + 1–5** | Assign a control group |
| **1–5** | Recall and focus a control group |
| **WASD / screen edge** | Pan tactical camera |
| **Mouse wheel** | Zoom around cursor |
| **Minimap click** | Move the tactical camera |
| **Esc** | Tactical pause |

On launch, `Up/Down` (or `Left/Right`) pick a mission from the mission-select
screen and `Space/Enter` deploy — mission 3, "A Voice in Conduit Twelve,"
unlocks after completing "Reclaim the Reactor." During mission 3, `K` near
the extraction console (while an Engineer is selected and in range) wakes
Lumen.

During the mission briefing, `Z`, `X`, and `C` purchase permanent Field Optics,
Reactive Plating, and Fabricator Overclock upgrades with campaign Lumen. `V`
cycles Ivo's field module and `N` cycles Sena's sensor module; both loadouts save
immediately. `M` cycles Mara's command doctrine and `O` cycles Olan's analysis
package. After establishing contact with Lumen, `L` cycles the relationship-gated
Guardian and Witness protocols. Later campaign decisions unlock `P` for Meridian
Bastion/Charter accords and `G` for Verdant Bloom/Briar covenants.

Select the Engineer and command it near each of the three power relays. Active
relays generate salvage for the Lantern fabricator; build reinforcements, hold
the sector, defeat the Choir Canticle, and bring the auxiliary reactor online.

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
| Feature-gated 3D mesh pipeline (depth-tested, single-light PBR) | ✅ core |
| RTS selection, formations, orders, navigation, fog | ✅ |
| RTS economy, production queues, power graphs, control groups | ✅ |
| Placement validation, tactical minimap, per-unit clip players | ✅ |
| Generated Engineer, Surveyor, and Choir action strips | ✅ |
| Full-roster hit reactions and persistent shutdown wrecks | ✅ |
| Four persistent named-specialist doctrine/loadout pairs | ✅ |
| Relationship-gated Lumen Guardian/Witness protocols | ✅ |
| Meridian and Verdant alliance doctrine foundations | ✅ |
| Versioned campaign progress with v1 save migration | ✅ |
| Data-driven mission definitions + in-game mission select | ✅ 2-mission campaign |
| Generic `SimpleAggroAi` (target scoring, retreat, focus-fire, `NavGrid` pathing) | ✅ |
| Skirmish free-play mode (reuses RTS core, no campaign/art dependency) | ✅ |
| Last Light point-and-click campaign missions | ✅ 2-mission vertical slice |

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
cd examples/last_light && trunk serve
```

## MCP support for coding agents

Aurora includes a local, repository-scoped MCP server for model-assisted engine
work. It provides a systems map, bounded source slices, the Last Light playtest
contract, and explicitly selected Cargo validation lanes—never arbitrary shell
commands or arbitrary filesystem reads. See [mcp/README.md](mcp/README.md) for
installation, client configuration, security boundaries, and the stdio protocol
smoke test.

## License

MIT OR Apache-2.0 · Repo: https://github.com/Sappgulf/aurora-engine
