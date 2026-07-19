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
| **Right click** | Move or attack contextually |
| **WASD / screen edge** | Pan tactical camera |
| **Mouse wheel** | Zoom around cursor |
| **Esc** | Tactical pause |

Select the Engineer and command it near each of the three power relays. Hold the
sector, defeat the Choir Canticle, and bring the auxiliary reactor online.

### Campaign and production guide

- [Campaign bible](docs/AURORA_LAST_LIGHT_CAMPAIGN.md)
- [Asset production guide](docs/AURORA_LAST_LIGHT_ASSET_GUIDE.md)

## Other demos

```bash
cargo run -p playground      # free-roam particles / camera
cargo run -p aurora_run      # earlier arcade vertical slice
cargo run -p triangle_demo   # M0 NDC triangle
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
| Feature-gated 3D mesh/material contract | ✅ foundation |
| RTS selection, formations, orders, navigation, fog | ✅ |
| Last Light point-and-click campaign mission | ✅ vertical slice |

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
