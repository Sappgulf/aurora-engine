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
- [x] Mini-game **Aurora Run** (`games/aurora-run`)
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
- [x] Playable **Reclaim the Reactor** mission (`games/last-light`)
- [x] Resource economy, production queues, and connected power-network graph
- [x] Control groups and viewport-anchored command-card UI
- [x] Versioned campaign persistence and mission unlock state
- [x] Per-entity clip state machine and Warden movement strip
- [x] Controller-first campaign flow with virtual tactical cursor, contextual
      command focus, unit cycling, objective focus, and haptic feedback
- [x] Engineer move, Surveyor scan, and Choir Needle attack strips
- [x] Canticle command and Bell Mine arming action strips
- [x] Hit/down reaction atlases for the full six-unit roster
- [x] Authored per-weapon pulse cadence with charge telegraphs, moving bolts,
      impact lighting, combat audio gating, haptics, and destruction debris
- [x] Generated "Terms of Salvage" vault plate with ten cover pylons,
      web-budgeted runtime export, and segmented navigation-matched bulkheads
- [x] Player-authored field-beacon placement and validation previews
- [x] Tactical minimap with fog, contacts, camera frame, and navigation
- [x] Combat framing assist for selected nearby engagements plus fog-safe
      off-screen contact/raid chevrons
- [x] Shared viewport-aware HUD layout contract with aligned lower rail,
      telemetry/comms rhythm, and DPR-stable safe-area spacing
- [x] Last Light sparse deterministic ambient mission bed layered through the
      engine sequencer so combat and radio cues retain foreground priority
- [x] Frame-faithful browser mouse/scroll injection, game-specific actions,
      and live combat engagement observability at 1×/2× DPR
- [x] Persistent three-branch campaign upgrade foundation
- [x] Persistent Ivo and Sena specialist loadouts with mission effects
- [x] Persistent Olan analysis and Mara command doctrines
- [x] Lumen-contact-gated Guardian and Witness ability choices
- [x] Meridian and Verdant alliance-specific late-campaign abilities
- [x] Data-driven `MissionDef`/`VictoryCondition` + in-game mission select
- [x] Mission 3 "A Voice in Conduit Twelve" (escort objective, corridor `NavGrid`
      obstacles, Lumen-wake choice)
- [x] Generic `SimpleAggroAi` (target scoring, retreat, focus-fire spread,
      `NavGrid`-routed approach) shared by the campaign and skirmish mode
- [x] Skirmish free-play mode (`demos/skirmish`) reusing RTS core systems
      without campaign save or authored art

**Exit criteria:** select and command a Lantern squad, reveal the sector, restore
three relays, defeat a Choir command unit, and complete the mission on native
and web builds.

**Test:** `cargo run -p last_light` (mission select → Reclaim the Reactor →
A Voice in Conduit Twelve), `cargo run -p skirmish`

---

## Milestone 4 — 3D path (feature-gated)

- [x] Perspective camera conventions + projection contract (`3d` feature)
- [x] Depth buffer + mesh pipeline
- [x] glTF mesh loader (GLB + embedded buffers, node transforms, PBR materials)
- [x] PBR materials (base color, metal, roughness)
- [x] Directional light (no shadow map yet)
- [x] Directional-light shadow maps (depth pass + PCF)
- [x] Gradient sky background pass with sun disk + hemispheric ambient
- [ ] Sky / IBL (image-based)

**Exit criteria:** One glTF model lit and orbit-controlled, web + native.

**Test:** `cargo run -p mesh_demo` (procedural cube + sphere, orbiting camera,
single-light PBR, depth-tested against `feature = "3d"`).

---

## Milestone 5 — Engine productization

- [ ] Scene graph or lightweight ECS (`hecs` re-export shipped behind the
      `ecs` feature; deeper integration open)
- [x] Hot reload shaders (native; `Renderer::reload_shaders` + WGSL override
      directory, validation-safe with per-pipeline rollback)
- [ ] Feature-gated modules (`3d` and `ecs` gated today; audio/2d split open)
- [x] Size budgets for Last Light and Platformer WASM plus source PNG payloads
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
├── games/                   # game-owned rules, saves, assets, and presentation
├── demos/                   # small public-API proofs and smoke tests
├── scripts/                 # build-web.sh, run-native.sh
└── ROADMAP.md               # this file
```

**Platform split:** `cfg(target_arch = "wasm32")` for logging, async device init, and canvas binding only. Rendering stays one pipeline.

---

## Milestone 7 — Platformer & action foundation ✅

- [x] Swept kinematic physics (`physics2d`): two-pass axis resolution, one-way
  ledges with crossing rule, game-owned moving platforms with rider carry,
  tilemap solids, tunnel-proof at any speed
- [x] Character controller: coyote time, jump buffering, variable jump
  height (release cut), apex hang, wall slide cap, wall jumps with steering
  lock
- [x] Gameplay probes: `ground_probe`, `raycast_any` with surface normals
- [x] Camera rig look-ahead with velocity lead and decay-to-center
- [x] `platformer` demo proving the full pack (crystals, ferry, wall-jump shaft)

**Exit criteria:** a second genre ships from the shared engine without touching
RTS code. ✅
**Test:** `cargo run -p platformer`

---

## Milestone 8 — Juice & structure kit ✅

- [x] 15 easing curves with anchored endpoints and documented overshooters
- [x] Copy-cheap `Tween<f32/Vec2/Color>` with delay/ease/Once/Repeat/PingPong,
  plus a tagged `TweenRunner` that prunes finished effects
- [x] Deterministic `Scheduler` (`after` / `every` / cancel) firing in
  deadline-then-id order for replay parity
- [x] `HitStop` time-freeze with stacking cap and surplus release
- [x] Seamless `parallax_offset` for wrapping background bands
- [x] Flat-table `StateMachine<S, E>` with bounded trace and rejected unknown
  events — platformer gate now drives its win banner through it

**Exit criteria:** game feel is a library problem, not per-project glue. ✅
**Test:** `cargo test -p aurora-engine -- juice fsm`

---

## Milestone 9 — Gamepad, data-driven levels, testable worlds ✅

- [x] `PadButton` standard mapping across `gilrs` (native) and Web Gamepad
  APIs; radial stick dead zone with rescale-from-zero; device precedence
  keyboard > d-pad > stick in `Input::move_axis`
- [x] `level` module: JSON `LevelDef`, validation errors naming the exact
  offending index, compiled `Level` (solids/one-ways/deterministic sin-wave
  movers/pickups/checkpoints/nav-grid bridge), authored solution routes and
  per-level player tuning
- [x] Platformer demo restructured into headless `GameCore`: window shell and
  the CI waypoint bot drive identical simulation
- [x] Playthrough harness: deterministic route-following bot completes the
  shipped "Crystal Run" level 6/6 crystals — geometry bugs unreachable by
  code review were found and fixed via this test alone
- [x] Scene parent-follow (`attach`/`detach`/`propagate`) with generation
  checks, cycle rejection, stale-link tolerance, multi-round settle tests

**Exit criteria:** worlds are data; shipping a level means its playthrough
test is green. ✅
**Test:** `cargo test -p platformer playthrough -- --nocapture`

### Still open (later)

- [x] Rumble/haptics through the pad surface (gilrs force feedback native,
      `vibrationActuator` dual-rumble on web; `Input::rumble` surface)
- [x] Multiple shipped levels + level-select flow sharing the bot harness
      (three levels: Crystal Run, Conduit Climb, Windlift — all bot-verified)
- [x] Level format tooling: hot-reload watch mode (`devtools::FileWatcher`
      + platformer integration)
- [x] Renderer-agnostic level authoring preview contract (`level_editor`):
      transactional edits, validation-gated compiled previews, bounded
      undo/redo, JSON export, and deterministic selection bounds
- [x] Checkpoint respawns wired into gameplay (`active_checkpoint` chain)
- [x] Platformer intent recorder + replay state-hash verification
- [ ] Full in-engine visual editor preview for level authoring

### Engine additions (v0.4 build-out)

- [x] Vector font text rendering: `font` module (ab_glyph rasterized atlas,
      layout with alignment/wrap/kerning, sprite bridge)
- [x] File audio playback: `Audio::play_music` / `play_sfx_file` via rodio
      with mixer-aware volume plumbing (native; beeps remain the web path)
- [x] Gamepad rumble: `Input::rumble`/`rumble_first` queue + gilrs effects
      and Web Gamepad haptics application in the app shell
- [x] glTF 2.0 loader (`gltf` module, `3d` feature) + mesh demo consumer
- [x] Shadow maps + gradient sky (`3d` feature)
- [x] Film grain post effect (`PostFxSettings::film_grain`)
- [x] `hecs` re-export behind the `ecs` feature
- [x] Platformer WASM/Trunk scaffold (`demos/platformer`)
- [x] Agent runtime control plane: loopback JSON-lines TCP server
      (`AURORA_AGENT_PORT`) + web JS bridge (`window.auroraInjectKey` /
      `auroraInjectPad` / `auroraState`), game hooks (`Game::agent_state`,
      `Game::on_agent_command`), level-validation CLI
      (`platformer --bin level-check`), and three MCP control tools
      (`aurora_playtest_platformer`, `aurora_validate_level`,
      `aurora_agent_control`) with a Playwright browser-agent spec

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
