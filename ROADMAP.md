# Aurora Engine — Goal Map

**Vision:** A fast Rust game engine with beautiful real-time graphics, one codebase for **desktop** and **browser** (WASM + WebGPU).

## North star

| Pillar | Target |
|--------|--------|
| **Language** | Rust |
| **Graphics** | wgpu → Vulkan / Metal / DX12 / WebGPU |
| **Platforms** | macOS, Windows, Linux, modern browsers |
| **Feel** | Thin, explicit engine — not a full Unity clone on day one |
| **Proof** | Real demos that ship natively *and* on the web |

---

## Milestone 0 — Bootstrap ✅ (this repo)

- [x] Cargo workspace (`aurora-engine` lib + `triangle_demo`)
- [x] Cross-platform window + event loop (`winit`)
- [x] GPU device / surface / render pipeline (`wgpu`)
- [x] WGSL triangle with time uniforms (spin + pulse)
- [x] `Game` trait + `run()` entrypoint
- [x] Native run path
- [x] WASM / Trunk web scaffold
- [x] README + scripts

**Test:** `cargo run -p triangle_demo` — rotating aurora triangle.

---

## Milestone 1 — Core 2D foundation

- [ ] Orthographic camera (zoom, pan)
- [ ] Sprite batcher (texture atlases, layering)
- [ ] Instantiated quads + basic materials
- [ ] Input map (keyboard / mouse / touch abstractions)
- [ ] Fixed timestep + variable render alpha
- [ ] Asset loader (images via `image` crate)
- [ ] Simple audio stub (cpal native / Web Audio later)

**Exit criteria:** A sprite-based mini scene runs native + web.

---

## Milestone 2 — “Beautiful graphics” pass

- [ ] HDR-ish color path + tonemapping
- [ ] Bloom / vignette / film grain post stack
- [ ] 2D lights (point + ambient) or SDF soft shadows
- [ ] Particle system (GPU or CPU)
- [ ] Text rendering (egui debug UI + game fonts)
- [ ] Screenshot / frame capture for CI

**Exit criteria:** Demo looks intentionally directed (palette, glow, motion).

---

## Milestone 3 — 3D path (optional branch)

- [ ] Perspective camera + depth buffer
- [ ] glTF mesh loader
- [ ] PBR materials (base color, metal, roughness)
- [ ] Directional light + shadow map
- [ ] Sky / IBL (simple)

**Exit criteria:** One glTF model lit and orbit-controlled, web + native.

---

## Milestone 4 — Engine productization

- [ ] Scene graph or lightweight ECS (`hecs` / custom)
- [ ] Hot reload shaders (native)
- [ ] Feature-gated modules (`2d`, `3d`, `audio`)
- [ ] Size budget for WASM (strip, LTO, asset streaming)
- [ ] CI: `cargo check` native + `wasm32`, clippy, fmt
- [ ] Published crates.io version `0.2`

---

## Milestone 5 — Vertical slice game

Ship a small complete game *in* Aurora (not a tech demo):

- Examples: twin-stick arena, endless flyer, puzzle grid, visual novel shell
- Shared save format, pause menu, settings
- Deploy static web build (GitHub Pages / Vercel)

---

## Non-goals (for now)

- Full visual editor (Godot-style) before runtime is solid
- Networking / multiplayer
- Mobile app stores as primary targets
- Competing with Bevy feature-for-feature

---

## Architecture snapshot

```
aurora-engine/
├── crates/aurora-engine/     # library: Game trait, Renderer, Time, Color
│   └── shaders/             # WGSL
├── examples/triangle_demo/  # smoke test (native + Trunk web)
├── scripts/                 # build-web.sh, run-native.sh
└── ROADMAP.md               # this file
```

**Platform split:** `cfg(target_arch = "wasm32")` for logging, async device init, and canvas binding only. Rendering stays one pipeline.

---

## Success metrics

1. `cargo run -p triangle_demo` works on your machine  
2. `trunk serve` shows the same demo in a browser  
3. Frame time headroom for 2D at 1080p (native)  
4. WASM download stays reasonable as features land (track size in CI later)

---

## Suggested next PR after M0

**M1.1 Camera + clear color API polish + keyboard toggle wireframe/debug overlay**  
Then **M1.2 textured quad**, then **sprite batch**.
