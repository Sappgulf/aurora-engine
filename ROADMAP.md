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

## Milestone 0 — Bootstrap ✅

- [x] Cargo workspace (`aurora-engine` lib + demos)
- [x] Cross-platform window + event loop (`winit`)
- [x] GPU device / surface / render pipeline (`wgpu`)
- [x] WGSL triangle with time uniforms (spin + pulse)
- [x] `Game` trait + `run()` entrypoint
- [x] Native run path
- [x] WASM / Trunk web scaffold
- [x] README + scripts

**Test:** `cargo run -p triangle_demo` — rotating aurora triangle.

---

## Milestone 1 — Core 2D foundation ✅

- [x] Orthographic camera (zoom, pan, screen↔world)
- [x] Sprite batcher (multi-texture runs, z-order, alpha blend)
- [x] Textured quads + vertex color tint
- [x] Input map (keyboard / mouse / scroll / WASD axis)
- [x] Fixed timestep + variable render (`Time::step_fixed`)
- [x] Texture API: RGBA, PNG bytes, procedural (circle, checker, gradient)
- [x] CPU particle system
- [x] `FrameCtx` unifies time + input + renderer
- [x] `playground` demo (default binary)
- [ ] Simple audio stub (cpal native / Web Audio later) → deferred to M2
- [ ] Touch abstractions → deferred to M2

**Exit criteria:** Sprite-based mini scene runs native (+ web scaffold). ✅  
**Test:** `cargo run -p playground`

---

## Milestone 2 — Polish + vertical slice ✅

- [x] Full-screen post stack (bloom threshold blur, vignette, chromatic)
- [x] Texture atlases + `Animation` frame playback
- [x] Audio beeps (rodio native, Web Audio wasm)
- [x] AABB collision
- [x] CPU particles (from M1)
- [x] Mini-game **Aurora Run** (`examples/aurora_run`)
- [ ] HDR / film grain / 2D lights (later)
- [ ] Text rendering / egui HUD (later)

**Exit criteria:** Playable collect/dodge game with post-FX + SFX. ✅  
**Test:** `cargo run -p aurora_run`

---

## Milestone 2.5 — World, feel, and shipping foundations

- [ ] Light components + composited 2D point-light pass
- [ ] Tile maps, collision layers, and environment props
- [ ] Lightweight scenes/entities and reusable movement/steering helpers
- [ ] Asset manifest with stable IDs for texture, audio, animation, and FX domains
- [ ] SDF/bitmap text plus compact HUD, pause, settings, and debug overlays
- [ ] Dash, hit-stop, knockback, combo, waves, and progression in Aurora Run
- [ ] Quality tiers, performance counters, and native/browser visual regression captures
- [ ] Saveable high score and deployed browser build

**Exit criteria:** Aurora Run is a 5–10 minute replayable vertical slice, and the
systems it uses can be adopted by a second 2D game without copying demo code.

---

## Milestone 3 — 3D path (feature-gated)

- [x] Perspective camera conventions + projection contract (`3d` feature)
- [ ] Depth buffer + mesh pipeline
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
