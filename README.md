# Aurora Engine

**Fast Rust game engine · beautiful wgpu graphics · desktop + browser**

Aurora is a from-scratch engine built on [wgpu](https://wgpu.rs) so the same renderer targets **Vulkan / Metal / DX12** natively and **WebGPU** in the browser via WebAssembly.

> Status: **Milestone 0** — bootstrap complete. Rotating triangle demo runs as a smoke test. See [ROADMAP.md](./ROADMAP.md) for the full goal map.

## Features (now)

- Cross-platform window + input loop (`winit` 0.30)
- GPU init, surface, WGSL pipelines (`wgpu` 24)
- `Game` trait: `on_start` / `on_update` / `on_event`
- Frame timing (`Time`: `delta`, `elapsed`, `frame`)
- Built-in **triangle demo** (aurora palette, spin + pulse)
- Web scaffold via [Trunk](https://trunkrs.dev)

## Quick start (native)

**Requirements:** Rust 1.75+ (edition 2021), a GPU with Vulkan/Metal/DX12.

```bash
git clone https://github.com/Sappgulf/aurora-engine.git
cd aurora-engine
cargo run -p triangle_demo
```

Or:

```bash
./scripts/run-native.sh
```

You should see a dark window with a **rotating teal / violet / magenta triangle**. Press **Esc** to quit.

## Browser (WebGPU)

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk   # once
cd examples/triangle_demo
trunk serve
```

Open the URL Trunk prints (default `http://127.0.0.1:8080`). Use a recent Chrome, Edge, Firefox, or Safari with WebGPU.

Release web build:

```bash
./scripts/build-web.sh
```

Output lands in `dist/`.

## Use as a library

```rust
use aurora_engine::{run, Color, Game, Renderer, Time};

struct MyGame;

impl Game for MyGame {
    fn name(&self) -> &str {
        "My Game"
    }

    fn on_update(&mut self, time: &Time, renderer: &mut Renderer) {
        let hue = (time.elapsed * 0.1) % 1.0;
        renderer.set_clear_color(Color::from_hue(hue).night_blend(0.8));
    }
}

fn main() {
    run(MyGame);
}
```

## Workspace layout

```
crates/aurora-engine/   # engine library
examples/triangle_demo/ # smoke-test binary (+ Trunk web entry)
shaders/                # (crate-local WGSL under crates/aurora-engine/shaders)
scripts/                # native + web helpers
ROADMAP.md              # milestones M0–M5
```

## Goal map (summary)

| Milestone | Focus |
|-----------|--------|
| **M0** ✅ | Loop, wgpu, triangle, native + web scaffold |
| **M1** | Camera, sprites, input, assets |
| **M2** | Post-FX, particles, “beautiful” look |
| **M3** | Optional 3D / glTF / PBR |
| **M4** | ECS, hot reload, CI, crates.io |
| **M5** | Vertical-slice game shipped in-engine |

Details: [ROADMAP.md](./ROADMAP.md).

## Why not Bevy?

Bevy is excellent. Aurora’s goal is a **small, readable engine** you fully own — ideal for learning GPU/engine structure and keeping WASM size intentional. You can still study Bevy for ECS and architecture ideas.

## License

MIT OR Apache-2.0
