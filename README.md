# Aurora Engine

**Fast Rust game engine · beautiful wgpu graphics · desktop + browser**

> Status: **v0.3 / Milestone 2** — post-FX, atlases, audio, collision, playable mini-game.

## Play now

```bash
cd ~/dev/aurora-engine
cargo run -p aurora_run
```

### Aurora Run controls

| Input | Action |
|--------|--------|
| **WASD** / arrows | Move |
| **Space** | Dash (uses a charge) |
| Collect **gold crystals** | Score and build combo |
| Collect **green upgrade crystal** | Restore a life + dash charge |
| Avoid **red drones** | Lose lives |
| **Esc** | Pause / resume |
| **R** | Restart |
| **P** | Toggle bloom / vignette / chromatic |
| **Esc** | Quit |

Top-left teal pips = lives · gold crystals = score · green crystals = upgrades.
Clear a wave to advance; each wave adds more varied drones.

## Other demos

```bash
cargo run -p playground      # free-roam particles / camera
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
cd examples/aurora_run && trunk serve
```

## License

MIT OR Apache-2.0 · Repo: https://github.com/Sappgulf/aurora-engine
