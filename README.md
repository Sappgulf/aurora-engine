# Aurora Engine

**Fast Rust game engine · beautiful wgpu graphics · desktop + browser**

Aurora is a from-scratch engine on [wgpu](https://wgpu.rs): **Vulkan / Metal / DX12** natively and **WebGPU** in the browser via WebAssembly.

> Status: **Milestone 1** — 2D foundation. Camera, sprites, input, textures, particles, fixed timestep. See [ROADMAP.md](./ROADMAP.md).

## Features

| Area | Status |
|------|--------|
| Cross-platform window + loop (`winit`) | ✅ |
| wgpu device / surface / WGSL | ✅ |
| `Game` + `FrameCtx` API | ✅ |
| Orthographic **Camera2D** (pan / zoom / screen↔world) | ✅ |
| **Sprite** batching (multi-texture, alpha blend) | ✅ |
| Procedural **textures** + PNG load | ✅ |
| **Input** (keys, mouse, scroll, WASD axis) | ✅ |
| Fixed timestep + variable render | ✅ |
| CPU **particles** | ✅ |
| Debug NDC triangle (M0) | ✅ |
| WASM / Trunk scaffold | ✅ |

## Quick start (native)

```bash
git clone https://github.com/Sappgulf/aurora-engine.git
cd aurora-engine
cargo run -p playground
```

### Controls (playground)

| Input | Action |
|--------|--------|
| **WASD** / arrows | Move player |
| **Scroll** / `+` `-` | Zoom camera |
| **LMB** / Space | Particle burst |
| **RMB** drag | Pan camera |
| **T** | Toggle debug triangle |
| **R** | Reset camera + player |
| **Esc** | Quit (native) |

M0 triangle-only smoke test:

```bash
cargo run -p triangle_demo
```

## Browser (WebGPU)

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cd examples/playground
trunk serve
```

## Use as a library

```rust
use aurora_engine::{run, Color, FrameCtx, Game, Renderer, Sprite, Texture};
use glam::Vec2;

struct MyGame {
    tex: usize,
}

impl Game for MyGame {
    fn on_start(&mut self, renderer: &mut Renderer) {
        let tex = {
            let gpu = renderer.gpu();
            Texture::soft_circle(&gpu, 64, Color::AURORA_TEAL)
        };
        self.tex = renderer.add_texture(tex);
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let dir = ctx.input.axis_wasd();
        ctx.renderer.camera.pan(dir * 200.0 * ctx.time.delta);
        ctx.renderer.draw_sprite(
            self.tex,
            Sprite::new(Vec2::ZERO, Vec2::splat(64.0)),
        );
    }
}

fn main() {
    run(MyGame { tex: 0 });
}
```

## Workspace

```
crates/aurora-engine/     # engine library
  shaders/                # WGSL (sprite + triangle)
examples/playground/      # M1 showcase (default)
examples/triangle_demo/   # M0 smoke test
scripts/
ROADMAP.md
```

## Goal map

| Milestone | Focus |
|-----------|--------|
| **M0** ✅ | Loop, wgpu, triangle, native + web scaffold |
| **M1** ✅ | Camera, sprites, input, assets, particles |
| **M2** | Post-FX, bloom, more polish |
| **M3** | Optional 3D / glTF / PBR |
| **M4** | ECS, hot reload, CI, crates.io |
| **M5** | Vertical-slice game |

## License

MIT OR Apache-2.0
