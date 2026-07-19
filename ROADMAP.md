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

- [x] Light components + composited 2D point-light pass
- [x] Tile maps, collision layers, and trigger regions
- [x] Lightweight scenes/entities and reusable camera rig helpers
- [x] Asset manifest with stable IDs plus load-state queue
- [x] Bitmap text, title/pause/settings/results menus, and debug diagnostic snapshots
- [x] Dash, combo, waves, upgrades, and progression in Aurora Run
- [x] Quality tiers, performance counters, and native/browser visual regression captures
- [x] Save/settings and channel-aware audio mixer contracts

**Exit criteria:** Aurora Run is a 5–10 minute replayable vertical slice, and the
systems it uses can be adopted by a second 2D game without copying demo code.

---

## Milestone 3 — Aurora: Last Light RTS foundation 🚧

- [x] Point and box selection with faction filtering
- [x] Contextual move and attack orders with deterministic formation slots
- [x] Grid navigation and reusable fog-of-war state
- [x] Strategic pan/zoom camera bounded to the authored world
- [x] Authored campaign, factions, characters, mission arc, and asset guide
- [x] Production unit and structure atlases
- [x] Playable **Reclaim the Reactor** mission (`examples/last_light`)
- [x] Resource economy, production queues, and connected power-network graph
- [x] Control groups and viewport-anchored command-card UI
- [x] Versioned campaign persistence and mission unlock state
- [x] Per-entity clip state machine and Warden movement strip
- [x] Engineer move, Surveyor scan, and Choir Needle attack strips
- [x] Canticle command and Bell Mine arming action strips
- [x] Hit/down reaction atlases for the full six-unit roster
- [x] Player-authored field-beacon placement and validation previews
- [x] Tactical minimap with fog, contacts, camera frame, and navigation
- [x] Persistent three-branch campaign upgrade foundation
- [x] Persistent Ivo and Sena specialist loadouts with mission effects
- [x] Persistent Olan analysis and Mara command doctrines
- [x] Lumen-contact-gated Guardian and Witness ability choices
- [ ] Meridian and Verdant alliance-specific late-campaign abilities

**Exit criteria:** select and command a Lantern squad, reveal the sector, restore
three relays, defeat a Choir command unit, and complete the mission on native
and web builds.

**Test:** `cargo run -p last_light`

---

## Milestone 4 — 3D path (feature-gated)

- [x] Perspective camera conventions + projection contract (`3d` feature)
- [ ] Depth buffer + mesh pipeline
- [ ] glTF mesh loader
- [ ] PBR materials (base color, metal, roughness)
- [ ] Directional light + shadow map
- [ ] Sky / IBL (simple)

**Exit criteria:** One glTF model lit and orbit-controlled, web + native.

---

## Milestone 5 — Engine productization

- [ ] Scene graph or lightweight ECS (`hecs` / custom)
- [ ] Hot reload shaders (native)
- [ ] Feature-gated modules (`2d`, `3d`, `audio`)
- [ ] Size budget for WASM (strip, LTO, asset streaming)
- [ ] CI: `cargo check` native + `wasm32`, clippy, fmt
- [ ] Published crates.io version `0.2`

---

## Milestone 6 — Vertical slice game

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
