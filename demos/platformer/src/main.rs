//! Platformer: the engine's `physics2d` + `level` packs proven end-to-end.
//!
//! Simulation lives in [`game_core::GameCore`] (fully headless, no renderer),
//! so the playable window and the CI playthrough bot drive byte-identical
//! code. Three shipped levels, checkpoint respawns, replay verification, and
//! hot-reloadable level data. Collect every crystal to win.

use platformer::{art, game_core, save, ui_kit};

use game_core::{CoreIntent, GameCore, StepReport};
use glam::Vec2;
use winit::keyboard::KeyCode;

#[cfg(not(target_arch = "wasm32"))]
use aurora_engine::devtools::FileWatcher;
use aurora_engine::{
    juice::{parallax_offset, Easing, HitStop, Scheduler, Tween, TweenRunner},
    music::{Melody, Note, Sequencer},
    run, Aabb, AudioChannel, CameraRig, Color, FrameCtx, Game, PadButton, ParticleSystem,
    PointLight, Renderer, RngLite, Sprite, Texture, TextureAtlas, TextureHandle, XorShift32,
};

/// The shipped levels, authored as data and validated on load.
const LEVEL_JSONS: [&str; 5] = [
    include_str!("../levels/crystal-run.json"),
    include_str!("../levels/conduit-climb.json"),
    include_str!("../levels/windlift.json"),
    include_str!("../levels/skyline.json"),
    include_str!("../levels/core.json"),
];

const LEVEL_BLURBS: [&str; 5] = [
    "FERRIES  LEDGES  COYOTE AIR",
    "ONE-WAY LADDER  TRUST THE CLIMB",
    "RIDE THE WINDLIFT  MIND THE GAP",
    "RAMPS UP  RAMPS DOWN  SHIFT DASH",
    "STOMP THE CORE  3 TIMES",
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Screen {
    LevelSelect,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy)]
struct Ghost {
    position: Vec2,
    size: Vec2,
    frame: usize,
    life: f32,
}

pub struct PlatformerGame {
    screen: Screen,
    level_index: usize,
    select_cursor: usize,
    level_names: Vec<String>,

    core: GameCore,
    facing: f32,

    rig: CameraRig,
    crystals_held: u32,

    tweens: TweenRunner<f32>,
    ring_positions: Vec<Vec2>,
    clock: Scheduler,
    stop: HitStop,
    hint_visible: bool,

    particles: ParticleSystem,
    rng: XorShift32,
    trail_cooldown: u32,
    run_cycle: f32,
    ghosts: Vec<Ghost>,
    session_log: game_core::replay::ReplayLog,
    ghost_log: Option<Vec<game_core::CoreIntent>>,
    ghost_core: Option<GameCore>,
    ghost_tick: usize,
    debug_physics: bool,
    ambience: Vec<aurora_engine::RateEmitter>,
    sequencer: Sequencer,

    replay_recording: bool,
    replay_log: game_core::replay::ReplayLog,
    replay_recorded_hash: Option<u64>,
    replay_message: Option<(String, f32)>,
    win_reported: bool,
    new_best: bool,
    death_flash: f32,
    level_banner: f32,
    debug_overlay: bool,
    progress: save::Progress,

    #[cfg(not(target_arch = "wasm32"))]
    level_watcher: Option<FileWatcher>,

    tex_block: TextureHandle,
    tex_ledge: TextureHandle,
    tex_player: TextureHandle,
    tex_crystal: TextureHandle,
    tex_ferry: TextureHandle,
    tex_ui: TextureHandle,
    tex_flag: TextureHandle,
    tex_terrain: TextureHandle,
    terrain_uv_stone: (Vec2, Vec2),
    terrain_uv_ledge: (Vec2, Vec2),
    terrain_uv_ferry: (Vec2, Vec2),
    terrain_uv_spike: (Vec2, Vec2),
    terrain_uv_cloud: (Vec2, Vec2),
    tex_panel9: TextureHandle,
    atlas_walker: TextureAtlas,
    atlas_character: TextureAtlas,
    atlas_flag: TextureAtlas,
}

impl Default for PlatformerGame {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformerGame {
    pub fn new() -> Self {
        Self::from_level_index(0)
    }

    /// Test/bot entry point: identical shell over any validated level.
    pub fn from_level(level_json: &str) -> Self {
        let mut game = Self::from_level_index(0);
        game.core = GameCore::from_level_json(level_json).expect("shipped levels must validate");
        game
    }

    fn from_level_index(index: usize) -> Self {
        let level_names = LEVEL_JSONS
            .iter()
            .map(|json| {
                aurora_engine::LevelDef::from_json(json)
                    .ok()
                    .map(|def| def.name)
                    .unwrap_or_else(|| "UNKNOWN".to_owned())
            })
            .collect();
        Self {
            screen: Screen::LevelSelect,
            level_index: index,
            select_cursor: index,
            level_names,
            core: GameCore::from_level_json(LEVEL_JSONS[index])
                .expect("shipped levels must always validate"),
            facing: 1.0,
            rig: CameraRig::new(Vec2::ZERO),
            crystals_held: 0,
            tweens: TweenRunner::new(),
            ring_positions: Vec::new(),
            clock: Scheduler::new(),
            stop: HitStop::default(),
            hint_visible: true,
            particles: ParticleSystem::new(512),
            rng: XorShift32::new(777),
            trail_cooldown: 0,
            run_cycle: 0.0,
            ghosts: Vec::new(),
            session_log: game_core::replay::ReplayLog::new(),
            ghost_log: None,
            ghost_core: None,
            ghost_tick: 0,
            debug_physics: false,
            ambience: Vec::new(),
            sequencer: Sequencer::new(ambient_melody()),
            replay_recording: false,
            replay_log: game_core::replay::ReplayLog::new(),
            replay_recorded_hash: None,
            replay_message: None,
            win_reported: false,
            new_best: false,
            death_flash: 0.0,
            level_banner: 0.0,
            debug_overlay: false,
            progress: save::Progress::load(),
            #[cfg(not(target_arch = "wasm32"))]
            level_watcher: None,
            tex_block: TextureHandle::default(),
            tex_ledge: TextureHandle::default(),
            tex_player: TextureHandle::default(),
            tex_crystal: TextureHandle::default(),
            tex_ferry: TextureHandle::default(),
            tex_ui: TextureHandle::default(),
            tex_flag: TextureHandle::default(),
            tex_terrain: TextureHandle::default(),
            terrain_uv_stone: (Vec2::ZERO, Vec2::ONE),
            terrain_uv_ledge: (Vec2::ZERO, Vec2::ONE),
            terrain_uv_ferry: (Vec2::ZERO, Vec2::ONE),
            terrain_uv_spike: (Vec2::ZERO, Vec2::ONE),
            terrain_uv_cloud: (Vec2::ZERO, Vec2::ONE),
            tex_panel9: TextureHandle::default(),
            atlas_walker: TextureAtlas::new(TextureHandle::default(), 1, 1, Vec2::ONE),
            atlas_character: TextureAtlas::new(TextureHandle::default(), 1, 1, Vec2::ONE),
            atlas_flag: TextureAtlas::new(TextureHandle::default(), 1, 1, Vec2::ONE),
        }
    }

    /// Exposes the headless core to the playthrough test.
    pub fn core(&mut self) -> &mut GameCore {
        &mut self.core
    }

    fn start_level(&mut self, index: usize) {
        self.level_index = index;
        self.core = GameCore::from_level_json(LEVEL_JSONS[index])
            .expect("shipped levels must always validate");
        self.facing = 1.0;
        self.crystals_held = 0;
        self.ring_positions.clear();
        self.tweens = TweenRunner::new();
        self.particles = ParticleSystem::new(512);
        self.replay_recording = false;
        self.replay_log = game_core::replay::ReplayLog::new();
        self.replay_recorded_hash = None;
        self.replay_message = None;
        self.win_reported = false;
        self.new_best = false;
        self.death_flash = 0.0;
        self.level_banner = 2.4;
        self.session_log = game_core::replay::ReplayLog::new();
        self.ghost_log = self
            .progress
            .ghosts
            .get(index)
            .map(<[game_core::CoreIntent]>::to_vec);
        self.ghost_core = self
            .ghost_log
            .as_ref()
            .map(|_| GameCore::from_level_json(LEVEL_JSONS[index]).expect("ghost level parses"));
        self.ghost_tick = 0;
        self.ambience = self
            .core
            .level
            .ambience
            .iter()
            .map(|def| {
                aurora_engine::RateEmitter::new(
                    def.rect.aabb().center(),
                    aurora_engine::EmitterConfig {
                        rate_per_sec: def.rate_per_sec,
                        speed: 14.0,
                        life: 2.2,
                        size: 7.0,
                        color: Color::rgb(
                            self.core.level.theme.particle[0],
                            self.core.level.theme.particle[1],
                            self.core.level.theme.particle[2],
                        ),
                        gravity_scale: -0.2,
                    },
                )
            })
            .collect();
        self.rig.bounds = Some(self.core.level.camera_bounds);
        self.screen = Screen::Playing;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn try_load_level_from_disk(&mut self) -> bool {
        let path = format!("levels/{}.json", self.core.level.id);
        let Ok(json) = std::fs::read_to_string(&path) else {
            return false;
        };
        match GameCore::from_level_json(&json) {
            Ok(core) => {
                log::info!("hot-reloaded level '{}' from {path}", core.level.name);
                self.core = core;
                true
            }
            Err(error) => {
                log::warn!("hot-reload rejected {path}: {error}");
                false
            }
        }
    }

    fn poll_level_hot_reload(&mut self, renderer: &mut Renderer) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let changed = self
                .level_watcher
                .as_mut()
                .is_some_and(|watcher| watcher.poll().is_some());
            if changed && self.try_load_level_from_disk() {
                self.rig.bounds = Some(self.core.level.camera_bounds);
                self.rig.snap_to_target(&mut renderer.camera);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = renderer;
    }

    fn draw_parallax(&self, renderer: &mut Renderer, elapsed: f32) {
        for (z, factor, span_x, count, tint, seed) in [
            (-7.0_f32, 0.15_f32, 480.0_f32, 14_usize, 0.30_f32, 101_u32),
            (-6.0_f32, 0.35_f32, 640.0_f32, 10_usize, 0.55_f32, 202_u32),
        ] {
            let view = renderer.camera.visible_world_size();
            let origin =
                parallax_offset(renderer.camera.position, factor, Vec2::new(span_x, view.y));
            let mut rng = XorShift32::new(seed);
            for _ in 0..count {
                let frac_x = rng.f32();
                let frac_y = rng.f32();
                let scrolled = (frac_x * span_x + origin.x).rem_euclid(span_x);
                let world_x = renderer.camera.position.x - view.x * 0.5 - span_x * 0.5 + scrolled;
                let world_y =
                    renderer.camera.position.y * (1.0 - factor) + view.y * (frac_y - 0.35);
                let twinkle = 0.72 + ((elapsed * 2.0 + frac_x * 12.56).sin() * 0.28);
                renderer.draw_sprite(
                    self.tex_player,
                    Sprite::new(Vec2::new(world_x, world_y), Vec2::splat(9.0))
                        .with_color(Color::rgba(1.0, 1.0, 1.0, tint * twinkle))
                        .with_z(z),
                );
            }
        }
    }

    /// Builds one frame of intent from raw device state. Keyboard wins over
    /// d-pad which wins over stick — mirrors `Input::move_axis` precedence so
    /// the playthrough bot (which injects pure stick values) exercises the
    /// same merge rules players do.
    fn gather_intent(&self, ctx: &FrameCtx<'_>) -> CoreIntent {
        let slot = ctx.input.first_pad();
        let axis = |neg: KeyCode, pos: KeyCode| -> f32 {
            match (ctx.input.key_down(neg), ctx.input.key_down(pos)) {
                (false, true) => 1.0,
                (true, false) => -1.0,
                _ => 0.0,
            }
        };
        let keyboard_axis = (axis(KeyCode::KeyA, KeyCode::KeyD)
            + axis(KeyCode::ArrowLeft, KeyCode::ArrowRight))
        .clamp(-1.0, 1.0);
        let dpad_right =
            slot.is_some_and(|slot| ctx.input.pad_button_down(slot, PadButton::DpadRight));
        let dpad_left =
            slot.is_some_and(|slot| ctx.input.pad_button_down(slot, PadButton::DpadLeft));
        let move_x = if keyboard_axis != 0.0 {
            keyboard_axis
        } else if dpad_right {
            1.0
        } else if dpad_left {
            -1.0
        } else {
            slot.map_or(0.0, |slot| ctx.input.pad_left_stick(slot).x)
        };

        let jump_pressed = ctx.input.key_pressed(KeyCode::Space)
            || ctx.input.key_pressed(KeyCode::KeyW)
            || slot.is_some_and(|slot| ctx.input.pad_button_pressed(slot, PadButton::South));
        let jump_held = ctx.input.key_down(KeyCode::Space)
            || ctx.input.key_down(KeyCode::KeyW)
            || slot.is_some_and(|slot| ctx.input.pad_button_down(slot, PadButton::South));
        let drop_request = (ctx.input.key_down(KeyCode::KeyS)
            || ctx.input.key_down(KeyCode::ArrowDown))
            && jump_pressed;
        let dash = ctx.input.key_pressed(KeyCode::ShiftLeft)
            || ctx.input.key_pressed(KeyCode::ShiftRight)
            || ctx.input.key_pressed(KeyCode::KeyX)
            || slot.is_some_and(|slot| ctx.input.pad_button_pressed(slot, PadButton::East));
        CoreIntent {
            move_x,
            jump_pressed,
            jump_held,
            self_drop: drop_request,
            dash,
        }
    }

    fn play_report_juice(&mut self, ctx: &mut FrameCtx<'_>, report: &StepReport) {
        let body_pos = self.core.body.position;
        let feet = body_pos - Vec2::Y * 28.0;
        if report.landed_hard {
            self.rig.shake(9.0, 0.22);
            self.stop.freeze(0.05);
            ctx.audio.beep_on(AudioChannel::Sfx, 130.0, 0.06, 0.32);
            self.particles.emit_burst(
                feet,
                14,
                130.0,
                0.4,
                11.0,
                Color::rgba(0.75, 0.78, 0.85, 0.9),
                &mut self.rng,
            );
            ctx.input.rumble_first(0.5, 0.2, 0.18);
        } else if report.jumped {
            ctx.audio.beep_on(AudioChannel::Sfx, 470.0, 0.08, 0.22);
            self.particles.emit_burst(
                feet,
                6,
                70.0,
                0.3,
                9.0,
                Color::rgba(0.8, 0.85, 0.95, 0.7),
                &mut self.rng,
            );
        }
        if let Some(index) = report.picked_up {
            self.crystals_held += 1;
            let at = self.core.level.pickups[index];
            self.ring_positions.push(at);
            self.rig.shake(4.0, 0.15);
            self.stop.freeze(0.04);
            ctx.audio.beep_on(AudioChannel::Sfx, 880.0, 0.07, 0.28);
            ctx.audio.beep_on(AudioChannel::Sfx, 1318.5, 0.1, 0.2);
            self.particles.emit_burst(
                at,
                16,
                190.0,
                0.55,
                10.0,
                Color::rgba(0.42, 0.95, 0.9, 1.0),
                &mut self.rng,
            );
            ctx.input.rumble_first(0.15, 0.35, 0.12);
            let tag = self.crystals_held as u64 - 1;
            self.tweens.start(
                tag,
                Tween::new(0.0_f32, 1.0)
                    .duration(0.45)
                    .ease(Easing::QuadOut),
            );
        }
        if let Some(index) = report.checkpoint_reached {
            let at = self.core.level.checkpoints[index];
            ctx.audio.beep_on(AudioChannel::Ui, 660.0, 0.09, 0.24);
            ctx.audio.beep_on(AudioChannel::Ui, 990.0, 0.14, 0.18);
            self.particles.emit_burst(
                at + Vec2::Y * 20.0,
                10,
                120.0,
                0.5,
                10.0,
                Color::rgba(1.0, 0.78, 0.3, 0.95),
                &mut self.rng,
            );
            ctx.input.rumble_first(0.2, 0.5, 0.15);
        }
        if let Some(index) = report.stomped {
            let at = self.core.enemy_bounds[index].center();
            self.rig.shake(5.0, 0.14);
            self.stop.freeze(0.035);
            ctx.audio.beep_on(AudioChannel::Sfx, 520.0, 0.07, 0.26);
            ctx.audio.beep_on(AudioChannel::Sfx, 660.0, 0.09, 0.18);
            self.particles.emit_burst(
                at,
                14,
                200.0,
                0.5,
                11.0,
                Color::rgba(0.72, 0.5, 0.95, 1.0),
                &mut self.rng,
            );
            ctx.input.rumble_first(0.3, 0.5, 0.14);
        }
        if report.dash_started {
            ctx.audio.beep_on(AudioChannel::Sfx, 300.0, 0.07, 0.22);
            ctx.audio.beep_on(AudioChannel::Sfx, 420.0, 0.05, 0.14);
            ctx.input.rumble_first(0.25, 0.4, 0.12);
            let frame = if self.core.dash_direction < 0.0 {
                -2
            } else {
                2
            };
            self.ghosts.push(Ghost {
                position: self.core.body.position,
                size: Vec2::new(104.0 * frame as f32, 104.0),
                frame: 8,
                life: 0.28,
            });
            self.rig.shake(2.0, 0.1);
        }
        if report.died {
            self.rig.shake(12.0, 0.3);
            self.death_flash = 1.0;
            ctx.audio.beep_on(AudioChannel::Sfx, 110.0, 0.22, 0.34);
            self.particles.emit_burst(
                body_pos,
                20,
                220.0,
                0.6,
                12.0,
                Color::rgba(0.95, 0.4, 0.35, 1.0),
                &mut self.rng,
            );
            ctx.input.rumble_first(0.8, 0.4, 0.3);
        }
    }
}

impl Game for PlatformerGame {
    fn name(&self) -> &str {
        "Aurora Platformer — Physics Pack Demo"
    }

    fn agent_state(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "screen": match self.screen {
                Screen::LevelSelect => "level_select",
                Screen::Playing => "playing",
                Screen::Paused => "paused",
            },
            "level": self.core.level.id,
            "level_index": self.level_index,
            "position": [self.core.body.position.x, self.core.body.position.y],
            "velocity": [self.core.body.velocity.x, self.core.body.velocity.y],
            "on_ground": self.core.body.on_ground,
            "collected": self.core.collected_count,
            "total": self.core.collected.len(),
            "checkpoints_hit": self.core.checkpoints_hit.iter().filter(|hit| **hit).count(),
            "elapsed": self.core.elapsed,
            "ticks": self.core.ticks,
            "won": self.core.won(),
            "hash": format!("{}", self.core.state_hash()),
        }))
    }

    fn on_agent_command(
        &mut self,
        action: &str,
        args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        match action {
            "reset" => {
                self.start_level(self.level_index);
                Some(serde_json::json!({"reset": self.core.level.id}))
            }
            "load_level" => {
                let index = args.get("index").and_then(serde_json::Value::as_u64)?;
                if index as usize >= LEVEL_JSONS.len() {
                    return Some(serde_json::json!({"error": format!(
                        "level index {index} out of range 0..{}",
                        LEVEL_JSONS.len()
                    )}));
                }
                let index = index as usize;
                self.select_cursor = index;
                self.start_level(index);
                Some(serde_json::json!({"loaded": self.core.level.id, "index": index}))
            }
            "teleport" => {
                let x = args.get("x").and_then(serde_json::Value::as_f64)?;
                let y = args.get("y").and_then(serde_json::Value::as_f64)?;
                self.core.teleport(Vec2::new(x as f32, y as f32));
                Some(serde_json::json!({"teleported": [x, y]}))
            }
            _ => None,
        }
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        renderer.camera.zoom = 0.8;
        // Softer aberration keeps bitmap HUD text crisp while sprites bloom.
        renderer.post_fx.chromatic = 0.0015;
        let (block, ledge, ferry, player, crystal, ui) = {
            let gpu = renderer.gpu();
            (
                Texture::solid(&gpu, Color::rgb(0.20, 0.28, 0.40)),
                Texture::solid(&gpu, Color::rgb(0.45, 0.72, 0.50)),
                Texture::solid(&gpu, Color::rgb(0.70, 0.46, 0.84)),
                Texture::soft_circle(&gpu, 96, Color::rgb(0.98, 0.82, 0.35)),
                Texture::crystal(&gpu, 64, Color::rgb(0.42, 0.95, 0.90)),
                Texture::solid(&gpu, Color::WHITE),
            )
        };
        self.tex_block = renderer.add_texture(block);
        self.tex_ledge = renderer.add_texture(ledge);
        self.tex_ferry = renderer.add_texture(ferry);
        self.tex_player = renderer.add_texture(player);
        self.tex_crystal = renderer.add_texture(crystal);
        self.tex_ui = renderer.add_texture(ui);

        // Procedural art pack (demos/platformer/src/art.rs).
        let (char_pixels, char_w, char_h, _cell) = art::character_strip();
        let character_texture = {
            let gpu = renderer.gpu();
            Texture::from_rgba(&gpu, char_w, char_h, &char_pixels, "character strip")
        };
        let character_handle = renderer.add_texture(character_texture);
        self.atlas_character = TextureAtlas::new(
            character_handle,
            art::CHARACTER_FRAMES as u32,
            1,
            Vec2::new(char_w as f32, char_h as f32),
        );

        let (flag_pixels, flag_w, flag_h, _flag_cell) = art::flag_pair();
        let flag_texture = {
            let gpu = renderer.gpu();
            Texture::from_rgba(&gpu, flag_w, flag_h, &flag_pixels, "flag pair")
        };
        let flag_handle = renderer.add_texture(flag_texture);
        self.atlas_flag =
            TextureAtlas::new(flag_handle, 2, 1, Vec2::new(flag_w as f32, flag_h as f32));
        self.tex_flag = flag_handle;

        // Runtime atlas packing: one texture + bind group for all terrain
        // pieces (fewer draw-call state changes than four separate binds).
        let (stone_px, stone_w, stone_h) = art::stone_tile();
        let (ledge_px, ledge_w, ledge_h) = art::ledge_tile();
        let (ferry_px, ferry_w, ferry_h) = art::ferry_tile();
        let (spike_px, spike_w, spike_h) = art::spike_tile();
        let (cloud_px, cloud_w, cloud_h) = art::cloud();
        let packed = aurora_engine::PackedAtlas::pack(&[
            ("stone", &stone_px, stone_w, stone_h),
            ("ledge", &ledge_px, ledge_w, ledge_h),
            ("ferry", &ferry_px, ferry_w, ferry_h),
            ("spike", &spike_px, spike_w, spike_h),
            ("cloud", &cloud_px, cloud_w, cloud_h),
        ])
        .expect("terrain atlas packs");
        let terrain_texture = {
            let gpu = renderer.gpu();
            Texture::from_rgba(
                &gpu,
                packed.width,
                packed.height,
                &packed.pixels,
                "terrain atlas",
            )
        };
        let terrain_handle = renderer.add_texture(terrain_texture);
        let uv = |name: &str| {
            let entry = packed
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .expect("packed entry present");
            entry.uv(packed.width, packed.height)
        };
        self.terrain_uv_stone = uv("stone");
        self.terrain_uv_ledge = uv("ledge");
        self.terrain_uv_ferry = uv("ferry");
        self.terrain_uv_spike = uv("spike");
        self.terrain_uv_cloud = uv("cloud");
        self.tex_terrain = terrain_handle;

        let (panel9, panel9_w, panel9_h) = art::panel9_tile();
        let panel9_texture = {
            let gpu = renderer.gpu();
            Texture::from_rgba(&gpu, panel9_w, panel9_h, &panel9, "panel9")
        };
        self.tex_panel9 = renderer.add_texture(panel9_texture);

        let (walker_pixels, walker_w, walker_h, _walker_cell) = art::walker_pair();
        let walker_texture = {
            let gpu = renderer.gpu();
            Texture::from_rgba(&gpu, walker_w, walker_h, &walker_pixels, "walker pair")
        };
        let walker_handle = renderer.add_texture(walker_texture);
        self.atlas_walker = TextureAtlas::new(
            walker_handle,
            2,
            1,
            Vec2::new(walker_w as f32, walker_h as f32),
        );

        self.rig.bounds = Some(self.core.level.camera_bounds);
        self.rig.look_ahead = 170.0;
        self.rig.follow_speed = 7.0;
        self.rig.dead_zone = Vec2::new(24.0, 48.0);
        self.rig.snap_to_target(&mut renderer.camera);
        self.clock.every(10, 2.2, 2.2);

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.level_watcher = Some(FileWatcher::new(format!(
                "levels/{}.json",
                self.core.level.id
            )));
        }
    }

    fn on_fixed_update(&mut self, ctx: &mut FrameCtx<'_>) {
        for note in self.sequencer.tick(game_core::FIXED_DT) {
            let channel = if note.frequency < 200.0 {
                AudioChannel::Music
            } else {
                AudioChannel::Ambience
            };
            ctx.audio
                .beep_on(channel, note.frequency, note.duration, note.volume);
        }
        for fire in self.clock.tick(game_core::FIXED_DT) {
            match fire.id {
                10 => self.hint_visible = !self.hint_visible,
                // Victory arpeggio, scheduled note by note.
                20 => ctx.audio.beep_on(AudioChannel::Music, 523.25, 0.14, 0.22),
                21 => ctx.audio.beep_on(AudioChannel::Music, 659.25, 0.14, 0.22),
                22 => ctx.audio.beep_on(AudioChannel::Music, 783.99, 0.14, 0.22),
                23 => ctx.audio.beep_on(AudioChannel::Music, 1046.5, 0.22, 0.24),
                _ => {}
            }
        }
        self.level_banner = (self.level_banner - game_core::FIXED_DT).max(0.0);
        if let Some(message) = &mut self.replay_message {
            message.1 -= game_core::FIXED_DT;
            if message.1 <= 0.0 {
                self.replay_message = None;
            }
        }

        self.poll_level_hot_reload(ctx.renderer);

        if ctx.input.key_pressed(KeyCode::F3) {
            self.debug_overlay = !self.debug_overlay;
            ctx.audio.beep_on(
                AudioChannel::Ui,
                if self.debug_overlay { 700.0 } else { 500.0 },
                0.04,
                0.18,
            );
        }

        if self.stop.filter(game_core::FIXED_DT) <= 0.0 {
            return;
        }

        if self.screen == Screen::LevelSelect {
            let left = ctx.input.key_pressed(KeyCode::ArrowLeft)
                || ctx.input.key_pressed(KeyCode::KeyA)
                || ctx
                    .input
                    .first_pad()
                    .is_some_and(|slot| ctx.input.pad_button_pressed(slot, PadButton::DpadLeft));
            let right = ctx.input.key_pressed(KeyCode::ArrowRight)
                || ctx.input.key_pressed(KeyCode::KeyD)
                || ctx
                    .input
                    .first_pad()
                    .is_some_and(|slot| ctx.input.pad_button_pressed(slot, PadButton::DpadRight));
            if left {
                self.select_cursor =
                    (self.select_cursor + LEVEL_JSONS.len() - 1) % LEVEL_JSONS.len();
                ctx.audio.beep_on(AudioChannel::Ui, 220.0, 0.04, 0.2);
            }
            if right {
                self.select_cursor = (self.select_cursor + 1) % LEVEL_JSONS.len();
                ctx.audio.beep_on(AudioChannel::Ui, 220.0, 0.04, 0.2);
            }
            let confirm = ctx.input.key_pressed(KeyCode::Space)
                || ctx.input.key_pressed(KeyCode::Enter)
                || ctx
                    .input
                    .first_pad()
                    .is_some_and(|slot| ctx.input.pad_button_pressed(slot, PadButton::South));
            if confirm {
                ctx.audio.beep_on(AudioChannel::Ui, 660.0, 0.06, 0.24);
                self.start_level(self.select_cursor);
                self.screen = Screen::Playing;
                self.rig.snap_to_target(&mut ctx.renderer.camera);
            }
            return;
        }

        if self.screen == Screen::Paused {
            let resume = ctx.input.key_pressed(KeyCode::Escape)
                || ctx.input.key_pressed(KeyCode::KeyP)
                || ctx.input.key_pressed(KeyCode::Space);
            let retry = ctx.input.key_pressed(KeyCode::KeyR);
            let levels = ctx.input.key_pressed(KeyCode::KeyQ);
            if resume {
                self.screen = Screen::Playing;
                ctx.audio.beep_on(AudioChannel::Ui, 550.0, 0.05, 0.2);
            } else if retry {
                ctx.audio.beep_on(AudioChannel::Ui, 440.0, 0.05, 0.2);
                self.start_level(self.level_index);
            } else if levels {
                self.select_cursor = self.level_index;
                self.screen = Screen::LevelSelect;
                ctx.audio.beep_on(AudioChannel::Ui, 330.0, 0.05, 0.2);
            }
            return;
        }

        if self.core.won() {
            if !self.win_reported {
                self.win_reported = true;
                for (index, id) in [20_u64, 21, 22, 23].into_iter().enumerate() {
                    self.clock.after(id, index as f32 * 0.12);
                }
                ctx.input.rumble_first(0.4, 0.8, 0.4);
                self.new_best = self.progress.record(self.level_index, self.core.elapsed);
                if self.new_best && !self.session_log.is_empty() {
                    let replaced = self
                        .progress
                        .ghosts
                        .record(self.level_index, &self.session_log);
                    self.ghost_log = self
                        .progress
                        .ghosts
                        .get(self.level_index)
                        .map(<[game_core::CoreIntent]>::to_vec);
                    if replaced {
                        self.replay_message = Some(("GHOST UPDATED — RACE IT!".to_owned(), 3.0));
                    }
                }
                let center = self.core.body.position;
                let confetti = [
                    Color::rgb(0.42, 0.95, 0.9),
                    Color::rgb(1.0, 0.78, 0.3),
                    Color::rgb(0.7, 0.46, 0.84),
                    Color::rgb(0.98, 0.82, 0.35),
                ];
                for (burst, tint) in confetti.iter().enumerate() {
                    self.particles.emit_burst(
                        center + Vec2::new((burst as f32 - 1.5) * 40.0, 20.0),
                        14,
                        260.0,
                        0.9,
                        11.0,
                        Color::rgba(tint.r, tint.g, tint.b, 1.0),
                        &mut self.rng,
                    );
                }
            }
            let next = ctx.input.key_pressed(KeyCode::Enter)
                || ctx
                    .input
                    .first_pad()
                    .is_some_and(|slot| ctx.input.pad_button_pressed(slot, PadButton::Start));
            let retry = ctx.input.key_pressed(KeyCode::KeyR);
            let back = ctx.input.key_pressed(KeyCode::Escape);
            if next {
                let target = (self.level_index + 1) % LEVEL_JSONS.len();
                ctx.audio.beep_on(AudioChannel::Ui, 660.0, 0.06, 0.24);
                self.select_cursor = target;
                self.start_level(target);
                self.rig.snap_to_target(&mut ctx.renderer.camera);
            } else if retry {
                ctx.audio.beep_on(AudioChannel::Ui, 440.0, 0.05, 0.2);
                self.start_level(self.level_index);
                self.rig.snap_to_target(&mut ctx.renderer.camera);
            } else if back {
                self.select_cursor = self.level_index;
                self.screen = Screen::LevelSelect;
            }
            return;
        }

        if ctx.input.key_pressed(KeyCode::Escape) || ctx.input.key_pressed(KeyCode::KeyP) {
            self.screen = Screen::Paused;
            ctx.audio.beep_on(AudioChannel::Ui, 330.0, 0.05, 0.2);
            return;
        }

        if ctx.input.key_pressed(KeyCode::F9) {
            self.replay_recording = !self.replay_recording;
            if self.replay_recording {
                self.replay_log = game_core::replay::ReplayLog::new();
                self.replay_recorded_hash = None;
                self.replay_message = Some(("RECORDING INTENTS".to_owned(), 2.0));
                ctx.audio.beep_on(AudioChannel::Ui, 550.0, 0.05, 0.2);
            } else {
                self.replay_recorded_hash = Some(self.core.state_hash());
                self.replay_message =
                    Some((format!("RECORDED {} INTENTS", self.replay_log.len()), 2.0));
                ctx.audio.beep_on(AudioChannel::Ui, 350.0, 0.05, 0.2);
            }
        }
        if ctx.input.key_pressed(KeyCode::F10) {
            let Some(recorded_hash) = self.replay_recorded_hash else {
                self.replay_message = Some(("NO RECORDING — PRESS F9 FIRST".to_owned(), 2.0));
                ctx.audio.beep_on(AudioChannel::Ui, 240.0, 0.06, 0.2);
                return;
            };
            let log = self.replay_log.clone();
            let level_json = LEVEL_JSONS[self.level_index];
            let outcome = game_core::replay::replay(level_json, &log).ok();
            match outcome {
                Some(outcome) if outcome.final_hash == recorded_hash => {
                    self.replay_message = Some(("REPLAY OK — HASH MATCH".to_owned(), 3.0));
                    ctx.audio.beep_on(AudioChannel::Ui, 880.0, 0.08, 0.24);
                }
                _ => {
                    self.replay_message = Some(("REPLAY MISMATCH".to_owned(), 3.0));
                    ctx.audio.beep_on(AudioChannel::Ui, 180.0, 0.12, 0.26);
                }
            }
        }

        let mut intent = self.gather_intent(ctx);
        if intent.self_drop && self.core.body.on_ground {
            // Consumed by physics via the drop-through grace window.
            self.core.body.request_drop_through();
        }
        intent.self_drop = false; // flags live only in this scope

        if self.replay_recording {
            self.replay_log.record(intent);
        }
        self.session_log.record(intent);
        {
            let mut spawned = Vec::new();
            for emitter in &mut self.ambience {
                emitter.tick(game_core::FIXED_DT, &mut self.rng, &mut spawned);
            }
            for mote in spawned {
                self.particles.emit_single(aurora_engine::SpawnedParticle {
                    position: mote.position,
                    velocity: mote.velocity,
                    life: mote.life,
                    size: mote.size,
                    color: Color::rgb(
                        self.core.level.theme.particle[0],
                        self.core.level.theme.particle[1],
                        self.core.level.theme.particle[2],
                    ),
                });
            }
        }
        if let (Some(ghost_core), Some(ghost_log)) =
            (self.ghost_core.as_mut(), self.ghost_log.as_ref())
        {
            if !ghost_core.won() && self.ghost_tick < ghost_log.len() {
                ghost_core.advance(ghost_log[self.ghost_tick]);
                self.ghost_tick += 1;
            }
        }

        if intent.move_x != 0.0 && !self.core.controller.steering_locked() {
            self.facing = intent.move_x.signum();
        }

        let report = self.core.advance(intent);

        let speed = self.core.body.velocity.length();
        if (self.core.body.on_ground && speed > 140.0) || speed > 420.0 {
            self.trail_cooldown += 1;
            if self.trail_cooldown >= 5 {
                self.trail_cooldown = 0;
                self.particles.emit_trail(
                    self.core.body.position - Vec2::Y * 24.0,
                    Color::rgba(0.7, 0.75, 0.85, 0.5),
                    &mut self.rng,
                );
            }
        }

        self.play_report_juice(ctx, &report);

        if report.respawned {
            self.rig.snap_to_target(&mut ctx.renderer.camera);
        }
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let dt = ctx.time.delta;
        self.tweens.tick(dt);
        self.death_flash = (self.death_flash - dt * 2.5).max(0.0);
        let theme = &self.core.level.theme;
        ctx.renderer.set_clear_color(Color::rgb(
            theme.sky_bottom[0],
            theme.sky_bottom[1],
            theme.sky_bottom[2],
        ));
        ctx.renderer.post_fx.speed_streaks =
            (self.core.dash_remaining / game_core::DASH_DURATION).clamp(0.0, 1.0) * 0.8;
        if self.screen == Screen::LevelSelect {
            // Menu backdrop: drifting starfield only, no world bleed-through.
            let drift = Vec2::new(ctx.time.elapsed * 24.0, 0.0);
            self.rig.target = drift;
            self.rig.target_velocity = Vec2::new(24.0, 0.0);
            self.rig.update(&mut ctx.renderer.camera, dt);
            self.draw_parallax(ctx.renderer, ctx.time.elapsed);
            self.draw_hud(ctx);
            return;
        }
        self.rig.target = self.core.body.position;
        self.rig.target_velocity = self.core.body.velocity;
        self.rig.update(&mut ctx.renderer.camera, dt);
        if self.screen != Screen::LevelSelect {
            let speed = self.core.body.velocity.length();
            let target_zoom = 0.8 - (speed / 1800.0).clamp(0.0, 1.0) * 0.16;
            let zoom_dt = dt * 3.5;
            ctx.renderer.camera.zoom +=
                (target_zoom - ctx.renderer.camera.zoom).clamp(-zoom_dt, zoom_dt);
        }

        let pulse = 1.0 + (ctx.time.elapsed * 4.0).sin().abs() * 0.10;
        let elapsed = ctx.time.elapsed;
        self.draw_parallax(ctx.renderer, elapsed);
        self.draw_clouds(ctx.renderer, elapsed);
        self.draw_aurora_bands(ctx.renderer, elapsed);

        let theme = &self.core.level.theme;
        let terrain_tint = Color::rgb(
            theme.terrain_tint[0],
            theme.terrain_tint[1],
            theme.terrain_tint[2],
        );
        for rect in &self.core.level.solids {
            draw_tiled(
                ctx.renderer,
                self.tex_terrain,
                Vec2::new(64.0, 64.0),
                self.terrain_uv_stone.0,
                self.terrain_uv_stone.1,
                *rect,
                terrain_tint,
                -1.0,
            );
            // Lit top face: a bright strip where sky meets rock.
            let strip = Aabb::new(
                Vec2::new(rect.min.x, rect.max.y - 8.0),
                Vec2::new(rect.max.x, rect.max.y),
            );
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(strip.center(), strip.size())
                    .with_color(Color::rgba(0.62, 0.76, 0.95, 0.45))
                    .with_z(-0.95),
            );
        }
        for ledge in &self.core.level.one_ways {
            draw_tiled(
                ctx.renderer,
                self.tex_terrain,
                Vec2::new(64.0, 20.0),
                self.terrain_uv_ledge.0,
                self.terrain_uv_ledge.1,
                *ledge,
                terrain_tint,
                -1.0,
            );
        }
        for bounds in &self.core.mover_bounds {
            draw_tiled(
                ctx.renderer,
                self.tex_terrain,
                Vec2::new(96.0, 26.0),
                self.terrain_uv_ferry.0,
                self.terrain_uv_ferry.1,
                *bounds,
                terrain_tint,
                -1.0,
            );
            let strip = Aabb::new(
                Vec2::new(bounds.min.x, bounds.max.y - 6.0),
                Vec2::new(bounds.max.x, bounds.max.y),
            );
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(strip.center(), strip.size())
                    .with_color(Color::rgba(0.9, 0.75, 1.0, 0.4))
                    .with_z(-0.95),
            );
        }

        for slope in &self.core.level.slopes {
            draw_slope(
                ctx.renderer,
                self.tex_terrain,
                self.terrain_uv_stone,
                slope,
                terrain_tint,
                -0.98,
            );
        }

        for hazard in &self.core.level.hazards {
            draw_tiled(
                ctx.renderer,
                self.tex_terrain,
                Vec2::new(64.0, 24.0),
                self.terrain_uv_spike.0,
                self.terrain_uv_spike.1,
                *hazard,
                terrain_tint,
                -0.9,
            );
        }
        if let Some(boss) = &self.core.boss {
            if !boss.dead {
                let flash = boss.flash > 0.0;
                let tint = if flash {
                    Color::rgba(1.0, 1.0, 1.0, 0.9)
                } else {
                    Color::rgb(0.85, 0.3, 0.3)
                };
                let size = Vec2::splat(boss.def.size * 1.15);
                let frame = ((ctx.time.elapsed * 4.0) as u64) % 2;
                ctx.renderer.draw_sprite(
                    self.atlas_walker.texture,
                    self.atlas_walker
                        .sprite(self.core.boss_bounds.center(), size, frame as u32)
                        .with_color(tint)
                        .with_z(-0.35),
                );
                ctx.renderer.draw_light(PointLight::new(
                    self.core.boss_bounds.center(),
                    Color::rgb(0.9, 0.25, 0.25),
                    boss.def.size * 1.6,
                    0.4,
                ));
                // HP pips above the boss.
                for pip in 0..boss.hp {
                    let pip_x = self.core.boss_bounds.center().x - (boss.hp as f32 * 14.0) / 2.0
                        + pip as f32 * 14.0;
                    ctx.renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(
                            Vec2::new(pip_x, self.core.boss_bounds.max.y + 22.0),
                            Vec2::new(10.0, 10.0),
                        )
                        .with_color(Color::rgb(0.95, 0.35, 0.3))
                        .with_z(-0.3),
                    );
                }
            }
        }

        for (index, enemy) in self.core.enemy_bounds.iter().enumerate() {
            if self.core.enemies_dead[index] {
                continue;
            }
            let frame = ((ctx.time.elapsed * 6.0) as u64 + index as u64) % 2;
            ctx.renderer.draw_sprite(
                self.atlas_walker.texture,
                self.atlas_walker
                    .sprite(enemy.center(), Vec2::new(52.0, 52.0), frame as u32)
                    .with_z(-0.4),
            );
            ctx.renderer.draw_light(PointLight::new(
                enemy.center(),
                Color::rgb(0.6, 0.3, 0.9),
                70.0,
                0.22,
            ));
        }

        // Ghost racer: translucent replay of the best run.
        if let Some(ghost) = self.ghost_core.as_ref() {
            if !ghost.won() {
                let ghost_feet = ghost.body.position.y - 26.0;
                let ghost_sprite_y = ghost_feet - (22.0 / 64.0) * 104.0;
                ctx.renderer.draw_sprite(
                    self.atlas_character.texture,
                    self.atlas_character
                        .sprite(
                            Vec2::new(ghost.body.position.x, ghost_sprite_y),
                            Vec2::new(104.0, 104.0),
                            0,
                        )
                        .with_color(Color::rgba(0.42, 1.0, 0.91, 0.3))
                        .with_z(0.46),
                );
            }
        }

        for (index, position) in self.core.level.checkpoints.iter().enumerate() {
            let activated = self.core.checkpoints_hit[index];
            let tint = if activated { 1.0 } else { 0.35 };
            let frame = ((elapsed * (if activated { 6.0 } else { 2.0 })) as u64 % 2) as u32;
            ctx.renderer.draw_sprite(
                self.atlas_flag.texture,
                self.atlas_flag
                    .sprite(
                        *position + Vec2::new(6.0, 26.0),
                        Vec2::new(48.0, 72.0),
                        frame,
                    )
                    .with_color(Color::rgba(1.0, 1.0, 1.0, 0.55 + tint * 0.45))
                    .with_z(-0.5),
            );
        }

        for (index, crystal) in self
            .core
            .level
            .pickups
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.core.collected[*index])
        {
            let _ = index;
            ctx.renderer.draw_sprite(
                self.tex_crystal,
                Sprite::new(*crystal, Vec2::splat(52.0 * pulse))
                    .with_rotation(elapsed * 1.5)
                    .with_z(0.0),
            );
        }

        for (tag, position) in self.ring_positions.iter().enumerate() {
            if let Some(progress) = self.tweens.value(tag as u64) {
                let ring_size = Vec2::splat(48.0 + progress * 220.0);
                ctx.renderer.draw_sprite(
                    self.tex_crystal,
                    Sprite::new(*position, ring_size)
                        .with_color(Color::rgba(0.42, 0.95, 0.90, 1.0 - progress))
                        .with_z(0.4),
                );
            }
        }

        if self.screen != Screen::Paused {
            self.particles.update(dt);
            for ghost in &mut self.ghosts {
                ghost.life -= dt;
            }
            self.ghosts.retain(|ghost| ghost.life > 0.0);
        }
        let mut particle_sprites = Vec::with_capacity(self.particles.len());
        self.particles.collect_sprites(&mut particle_sprites);
        for sprite in particle_sprites {
            ctx.renderer.draw_sprite(self.tex_player, sprite);
        }

        // Character sprite: procedural atlas with idle/run/jump/fall poses,
        // velocity-driven squash, and a soft contact shadow.
        let on_ground = self.core.body.on_ground;
        let vx = self.core.body.velocity.x;
        let vy = self.core.body.velocity.y;
        if on_ground && vx.abs() > 20.0 {
            self.run_cycle += vx.abs() * dt / 46.0;
        }
        let moving = on_ground && vx.abs() > 20.0;
        let frame = if !on_ground {
            if vy > 0.0 {
                8
            } else {
                9
            }
        } else if moving {
            2 + (self.run_cycle * 6.0).fract() as usize % 6
        } else {
            (elapsed * 1.2) as usize % 2
        };
        let stretch = (vy.abs() / 1500.0).clamp(0.0, 0.24);
        let flip_x = if self.facing < 0.0 { -1.0 } else { 1.0 };
        let draw_size = Vec2::new(
            104.0 * (1.0 - stretch * 0.5) * flip_x,
            104.0 * (1.0 + stretch * 0.8),
        );
        // The art's ground line sits 22px below the 64px cell's center.
        let feet_world = self.core.body.position.y - 26.0;
        let sprite_y = feet_world - (22.0 / 64.0) * draw_size.y;
        if on_ground {
            ctx.renderer.draw_sprite(
                self.tex_player,
                Sprite::new(
                    Vec2::new(self.core.body.position.x, feet_world + 2.0),
                    Vec2::new(52.0, 12.0),
                )
                .with_color(Color::rgba(0.0, 0.0, 0.05, 0.3))
                .with_z(0.45),
            );
        }
        for ghost in &self.ghosts {
            let alpha = (ghost.life / 0.28).clamp(0.0, 1.0) * 0.35;
            let ghost_y = ghost.position.y - 26.0 - (22.0 / 64.0) * ghost.size.y.abs();
            ctx.renderer.draw_sprite(
                self.atlas_character.texture,
                self.atlas_character
                    .sprite(
                        Vec2::new(ghost.position.x, ghost_y),
                        ghost.size,
                        ghost.frame as u32,
                    )
                    .with_color(Color::rgba(0.42, 1.0, 0.91, alpha))
                    .with_z(0.48),
            );
        }
        // Dash cooldown pip under the player reads at a glance.
        let pip_alpha = if self.core.dash_cooldown <= 0.0 {
            0.85
        } else {
            0.15 + 0.1 * (ctx.time.elapsed * 8.0).sin()
        };
        let pip_color = Color::rgba(0.42, 1.0, 0.91, pip_alpha);
        ctx.renderer.draw_sprite(
            self.tex_ui,
            Sprite::new(
                Vec2::new(self.core.body.position.x, feet_world - 8.0),
                Vec2::new(18.0, 4.0),
            )
            .with_color(pip_color)
            .with_z(0.45),
        );
        ctx.renderer.draw_sprite(
            self.atlas_character.texture,
            self.atlas_character
                .sprite(
                    Vec2::new(self.core.body.position.x, sprite_y),
                    draw_size,
                    frame as u32,
                )
                .with_z(0.5),
        );

        // The engine's 2D light pass: the player carries a warm lantern,
        // crystals glow cyan, activated checkpoints hold an amber flame.
        let speed = self.core.body.velocity.length();
        let lantern = 0.30 + (speed / 900.0).clamp(0.0, 1.0) * 0.12;
        let accent = Color::rgb(theme.accent[0], theme.accent[1], theme.accent[2]);
        ctx.renderer.draw_light(PointLight::new(
            self.core.body.position,
            Color::rgb(1.0, 0.82, 0.5),
            150.0,
            lantern,
        ));
        for (index, crystal) in self
            .core
            .level
            .pickups
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.core.collected[*index])
        {
            let _ = index;
            ctx.renderer.draw_light(PointLight::new(
                *crystal,
                Color::rgb(
                    0.3 + accent.r * 0.4,
                    0.6 + accent.g * 0.4,
                    0.7 + accent.b * 0.3,
                ),
                95.0,
                0.32 + 0.1 * (elapsed * 3.0).sin(),
            ));
        }
        for (index, position) in self.core.level.checkpoints.iter().enumerate() {
            if self.core.checkpoints_hit[index] {
                ctx.renderer.draw_light(PointLight::new(
                    *position + Vec2::Y * 30.0,
                    Color::rgb(1.0, 0.72, 0.25),
                    120.0,
                    0.3 + 0.08 * (elapsed * 5.0).sin(),
                ));
            }
        }

        if self.death_flash > 0.0 {
            let view = ctx.renderer.camera.visible_world_size();
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(ctx.renderer.camera.position, view + Vec2::splat(80.0))
                    .with_color(Color::rgba(0.55, 0.1, 0.12, self.death_flash * 0.4))
                    .with_z(10.0),
            );
        }

        if self.debug_physics {
            let renderer = &mut *ctx.renderer;
            renderer.draw_debug_aabb(self.core.body.aabb(), Color::rgb(0.4, 1.0, 0.9));
            renderer.draw_debug_aabb(
                self.core.level.camera_bounds,
                Color::rgba(1.0, 0.8, 0.3, 0.5),
            );
            for enemy in &self.core.enemy_bounds {
                renderer.draw_debug_aabb(*enemy, Color::rgb(0.9, 0.4, 0.95));
            }
            for hazard in &self.core.level.hazards {
                renderer.draw_debug_aabb(*hazard, Color::rgb(1.0, 0.45, 0.3));
            }
            if let Some(boss) = &self.core.boss {
                if !boss.dead {
                    renderer.draw_debug_aabb(self.core.boss_bounds, Color::rgb(1.0, 0.3, 0.3));
                }
            }
            for water in &self.core.level.water {
                renderer.draw_debug_aabb(*water, Color::rgb(0.3, 0.6, 1.0));
            }
        }

        self.draw_hud(ctx);
    }
}

impl PlatformerGame {
    /// Two slow, hue-shifting light bands behind the starfield: the aurora
    /// the studio is named for.
    /// Mid-parallax drifting clouds between the stars and the aurora.
    fn draw_clouds(&self, renderer: &mut Renderer, elapsed: f32) {
        let view = renderer.camera.visible_world_size();
        let mut rng = XorShift32::new(303);
        for index in 0..4 {
            let seed_x = rng.f32();
            let seed_y = rng.f32();
            let span_x = 1400.0_f32;
            let factor = 0.45_f32 + index as f32 * 0.08;
            let drift = elapsed * (6.0 + index as f32 * 2.5);
            let origin =
                parallax_offset(renderer.camera.position, factor, Vec2::new(span_x, view.y));
            let world_x = renderer.camera.position.x - view.x * 0.5
                + (seed_x * span_x + origin.x - drift).rem_euclid(span_x + 600.0)
                - 300.0;
            let world_y =
                renderer.camera.position.y * (1.0 - factor) + view.y * (seed_y * 0.7 - 0.15);
            let scale = 1.6 + seed_x * 1.4;
            let mut cloud_sprite = Sprite::new(
                Vec2::new(world_x, world_y),
                Vec2::new(160.0 * scale, 64.0 * scale * 0.6),
            )
            .with_z(-5.0 - index as f32 * 0.1);
            cloud_sprite.uv_min = self.terrain_uv_cloud.0;
            cloud_sprite.uv_max = self.terrain_uv_cloud.1;
            renderer.draw_sprite(self.tex_terrain, cloud_sprite);
        }
    }

    fn draw_aurora_bands(&self, renderer: &mut Renderer, elapsed: f32) {
        let view = renderer.camera.visible_world_size();
        for (factor, span_x, tint, phase, height) in [
            (
                0.08_f32,
                900.0_f32,
                (0.25_f32, 0.9, 0.7),
                0.0_f32,
                260.0_f32,
            ),
            (0.12, 1200.0, (0.55, 0.35, 0.95), 2.1, 320.0),
        ] {
            let origin =
                parallax_offset(renderer.camera.position, factor, Vec2::new(span_x, view.y));
            let sway = (elapsed * 0.35 + phase).sin() * 60.0;
            let band_y =
                renderer.camera.position.y + view.y * (0.22 + 0.05 * (elapsed * 0.5 + phase).sin());
            let alpha = 0.05 + 0.025 * (elapsed * 0.8 + phase * 1.7).sin();
            renderer.draw_sprite(
                self.tex_player,
                Sprite::new(
                    Vec2::new(
                        renderer.camera.position.x - view.x * 0.5 - span_x * 0.5
                            + origin.x.rem_euclid(span_x)
                            + sway,
                        band_y,
                    ),
                    Vec2::new(span_x * 1.6, height),
                )
                .with_color(Color::rgba(tint.0, tint.1, tint.2, alpha))
                .with_z(-8.0),
            );
            renderer.draw_sprite(
                self.tex_player,
                Sprite::new(
                    Vec2::new(
                        renderer.camera.position.x - view.x * 0.5 - span_x * 0.5
                            + (origin.x + span_x * 0.5).rem_euclid(span_x)
                            - sway,
                        band_y + 40.0,
                    ),
                    Vec2::new(span_x * 1.4, height * 0.8),
                )
                .with_color(Color::rgba(tint.1, tint.0, tint.2, alpha * 0.8))
                .with_z(-8.0),
            );
        }
    }

    fn draw_hud(&self, ctx: &mut FrameCtx<'_>) {
        use ui_kit as kit;
        let camera = ctx.renderer.camera.clone();
        if self.screen == Screen::LevelSelect {
            // --- Level select: title, cards with best-time chips, footer --
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.86),
                "AURORA PLATFORMER",
                4.0,
                kit::TEAL,
                9.0,
            );
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.79),
                "SELECT A LEVEL",
                2.0,
                kit::INK_DIM,
                9.0,
            );
            let view = camera.visible_world_size();
            for (index, name) in self.level_names.iter().enumerate() {
                let selected = index == self.select_cursor;
                let card = aurora_engine::Aabb::new(
                    Vec2::new(camera.position.x - 330.0, 0.0),
                    Vec2::new(camera.position.x + 330.0, 0.0),
                );
                let card_top = camera.position.y + view.y * (0.645 - index as f32 * 0.135 - 0.5);
                let card_height = 84.0;
                let rect = aurora_engine::Aabb::new(
                    Vec2::new(card.min.x, card_top - card_height),
                    Vec2::new(card.max.x, card_top),
                );
                kit::panel9(ctx.renderer, self.tex_panel9, rect, 8.9);
                let text_y = rect.max.y - 34.0;
                let title = format!("0{}. {name}", index + 1);
                kit::text(
                    ctx.renderer,
                    self.tex_ui,
                    Vec2::new(rect.min.x + 34.0, text_y),
                    &title,
                    3.0,
                    if selected { kit::CYAN } else { kit::INK },
                    9.1,
                );
                let blurb = match self.progress.best(index) {
                    Some(best) => format!("{}   BEST {:.1}S", LEVEL_BLURBS[index], best),
                    None => LEVEL_BLURBS[index].to_owned(),
                };
                kit::text(
                    ctx.renderer,
                    self.tex_ui,
                    Vec2::new(rect.min.x + 34.0, text_y - 32.0),
                    &blurb,
                    2.0,
                    kit::INK_DIM,
                    9.1,
                );
                if selected {
                    kit::text(
                        ctx.renderer,
                        self.tex_ui,
                        Vec2::new(rect.min.x + 10.0, text_y),
                        ">",
                        3.0,
                        kit::TEAL,
                        9.2,
                    );
                }
            }
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.14),
                "A/D CHOOSE      SPACE START",
                2.0,
                kit::INK_DIM,
                9.0,
            );
            return;
        }

        if self.screen == Screen::Paused {
            let view = camera.visible_world_size();
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(camera.position, view + Vec2::splat(60.0))
                    .with_color(Color::rgba(0.02, 0.03, 0.09, 0.6))
                    .with_z(9.5),
            );
            let panel_width = 460.0_f32;
            let panel_height = 220.0_f32;
            let rect = aurora_engine::Aabb::new(
                Vec2::new(
                    camera.position.x - panel_width * 0.5,
                    camera.position.y - panel_height * 0.5,
                ),
                Vec2::new(
                    camera.position.x + panel_width * 0.5,
                    camera.position.y + panel_height * 0.5,
                ),
            );
            kit::panel9(ctx.renderer, self.tex_panel9, rect, 9.6);
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.595),
                "PAUSED",
                4.0,
                kit::TEAL,
                9.7,
            );
            let rows = [
                ("ESC / P", "RESUME"),
                ("R", "RETRY LEVEL"),
                ("Q", "LEVEL SELECT"),
            ];
            for (index, (key, label)) in rows.iter().enumerate() {
                let y = camera.position.y + 18.0 - index as f32 * 30.0;
                kit::text(
                    ctx.renderer,
                    self.tex_ui,
                    Vec2::new(camera.position.x - 170.0, y),
                    key,
                    2.0,
                    kit::AMBER,
                    9.7,
                );
                kit::text(
                    ctx.renderer,
                    self.tex_ui,
                    Vec2::new(camera.position.x - 60.0, y),
                    label,
                    2.0,
                    kit::INK,
                    9.7,
                );
            }
            return;
        }

        // --- Level intro banner -------------------------------------------
        if self.level_banner > 0.0 && !self.core.won() {
            let fade_in = ((2.4 - self.level_banner) / 0.25).clamp(0.0, 1.0);
            let fade_out = (self.level_banner / 0.6).clamp(0.0, 1.0);
            let alpha = fade_in.min(fade_out);
            let panel_width = 520.0_f32;
            let panel_height = 96.0_f32;
            let banner_view = camera.visible_world_size();
            // Panel centers on the midpoint between the title and subtitle
            // text anchors (0.62 and 0.575 viewport fractions).
            let banner_center_y = camera.position.y + banner_view.y * (0.5975 - 0.5);
            let rect = aurora_engine::Aabb::new(
                Vec2::new(
                    camera.position.x - panel_width * 0.5,
                    banner_center_y - panel_height * 0.5,
                ),
                Vec2::new(
                    camera.position.x + panel_width * 0.5,
                    banner_center_y + panel_height * 0.5,
                ),
            );
            kit::panel9(ctx.renderer, self.tex_panel9, rect, 9.0);
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.62),
                &self.core.level.name,
                3.0,
                Color::rgba(kit::CYAN.r, kit::CYAN.g, kit::CYAN.b, alpha),
                9.1,
            );
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.575),
                "COLLECT EVERY CRYSTAL",
                2.0,
                Color::rgba(kit::INK_DIM.r, kit::INK_DIM.g, kit::INK_DIM.b, alpha),
                9.1,
            );
        }

        // --- In-game HUD chips --------------------------------------------
        let collected = self.core.collected_count;
        let total = self.core.collected.len();
        kit::chip(
            ctx.renderer,
            self.tex_ui,
            &camera,
            Vec2::new(0.02, 0.035),
            &format!("{}  {collected}/{total}", self.core.level.name),
            kit::TEAL_DIM,
            9.0,
        );
        kit::chip(
            ctx.renderer,
            self.tex_ui,
            &camera,
            Vec2::new(0.86, 0.035),
            &format!("TIME {:.1}S", self.core.elapsed),
            kit::TEAL_DIM,
            9.0,
        );
        if self.hint_visible && !self.core.won() {
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.115),
                "A/D MOVE   SPACE JUMP   SHIFT DASH   S+SPACE DROP   ESC PAUSE",
                2.0,
                kit::INK_DIM,
                9.0,
            );
        }
        if let Some((message, _)) = &self.replay_message {
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.19),
                message,
                2.0,
                kit::CYAN,
                9.0,
            );
        }

        // --- Win overlay panel --------------------------------------------
        if self.core.won() {
            let view = camera.visible_world_size();
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(camera.position, view + Vec2::splat(60.0))
                    .with_color(Color::rgba(0.02, 0.03, 0.09, 0.55))
                    .with_z(9.5),
            );
            let panel_width = 620.0_f32;
            let panel_height = 250.0_f32;
            let rect = aurora_engine::Aabb::new(
                Vec2::new(
                    camera.position.x - panel_width * 0.5,
                    camera.position.y - panel_height * 0.5,
                ),
                Vec2::new(
                    camera.position.x + panel_width * 0.5,
                    camera.position.y + panel_height * 0.5,
                ),
            );
            kit::panel9(ctx.renderer, self.tex_panel9, rect, 9.6);
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.615),
                "LEVEL COMPLETE!",
                2.0,
                kit::AMBER,
                9.7,
            );
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.565),
                "ALL CRYSTALS RECOVERED",
                3.0,
                kit::CYAN,
                9.7,
            );
            let time_line = format!("TIME {:.1}S", self.core.elapsed);
            let best_line = match self.progress.best(self.level_index) {
                Some(best) => format!("BEST {:.1}S", best),
                None => "BEST --".to_owned(),
            };
            kit::text(
                ctx.renderer,
                self.tex_ui,
                Vec2::new(camera.position.x - 240.0, camera.position.y - 10.0),
                &time_line,
                3.0,
                kit::INK,
                9.7,
            );
            kit::text_right(
                ctx.renderer,
                self.tex_ui,
                camera.position.x + 240.0,
                camera.position.y - 10.0,
                &best_line,
                3.0,
                kit::INK,
                9.7,
            );
            if self.new_best {
                let badge_text = "NEW BEST!";
                let badge_width = kit::text_width(badge_text, 2.0) + 24.0;
                let badge_rect = aurora_engine::Aabb::new(
                    Vec2::new(
                        camera.position.x - badge_width * 0.5,
                        camera.position.y - 58.0,
                    ),
                    Vec2::new(
                        camera.position.x + badge_width * 0.5,
                        camera.position.y - 26.0,
                    ),
                );
                kit::panel9(ctx.renderer, self.tex_panel9, badge_rect, 9.7);
                kit::text(
                    ctx.renderer,
                    self.tex_ui,
                    Vec2::new(
                        badge_rect.center().x - kit::text_width(badge_text, 2.0) * 0.5,
                        badge_rect.min.y + 9.0,
                    ),
                    badge_text,
                    2.0,
                    kit::AMBER,
                    9.8,
                );
            }
            kit::text_centered(
                ctx.renderer,
                self.tex_ui,
                &camera,
                Vec2::new(0.5, 0.315),
                "ENTER NEXT    R RETRY    ESC LEVELS",
                2.0,
                kit::INK_DIM,
                9.7,
            );
        }

        if self.debug_overlay {
            let view = camera.visible_world_size();
            let snapshot = ctx.diagnostics.latest();
            let rows = [
                format!(
                    "FPS {:.0}  FRAME {:.2}MS",
                    ctx.diagnostics.smoothed_fps(),
                    ctx.diagnostics.smoothed_frame_ms()
                ),
                format!(
                    "POS {:?}  VEL {:?}",
                    [self.core.body.position.x, self.core.body.position.y],
                    [self.core.body.velocity.x, self.core.body.velocity.y]
                ),
                format!(
                    "TICK {}  HASH {:016x}",
                    self.core.ticks,
                    self.core.state_hash()
                ),
                format!(
                    "GROUNDS {}  CHECKPOINTS {}/{}  SPRITES {}",
                    self.core.body.on_ground,
                    self.core.checkpoints_hit.iter().filter(|hit| **hit).count(),
                    self.core.checkpoints_hit.len(),
                    snapshot.map(|s| s.drawn_sprites).unwrap_or(0)
                ),
            ];
            let overlay_width = 430.0_f32;
            let overlay_height = 16.0 + rows.len() as f32 * 22.0;
            let rect = aurora_engine::Aabb::new(
                Vec2::new(
                    camera.position.x - view.x * 0.5 + 16.0,
                    camera.position.y + view.y * 0.5 - 16.0 - overlay_height,
                ),
                Vec2::new(
                    camera.position.x - view.x * 0.5 + 16.0 + overlay_width,
                    camera.position.y + view.y * 0.5 - 16.0,
                ),
            );
            kit::panel(
                ctx.renderer,
                self.tex_ui,
                rect,
                Color::rgba(kit::AMBER.r, kit::AMBER.g, kit::AMBER.b, 0.5),
                9.9,
            );
            for (index, row) in rows.iter().enumerate() {
                kit::text(
                    ctx.renderer,
                    self.tex_ui,
                    Vec2::new(rect.min.x + 10.0, rect.max.y - 22.0 - index as f32 * 22.0),
                    row,
                    2.0,
                    kit::INK,
                    10.0,
                );
            }
        }
    }
}

/// Tiles `texture` across `rect` on its native pixel grid, sub-UV-ing the
/// edge cells so nothing stretches.
#[allow(clippy::too_many_arguments)] // Keeps terrain call sites readable.
fn draw_tiled(
    renderer: &mut Renderer,
    texture: TextureHandle,
    tile_size: Vec2,
    uv_min: Vec2,
    uv_max: Vec2,
    rect: Aabb,
    tint: Color,
    z: f32,
) {
    let span = uv_max - uv_min;
    let mut y = rect.min.y;
    while y < rect.max.y - 0.5 {
        let cell_h = tile_size.y.min(rect.max.y - y);
        let mut x = rect.min.x;
        while x < rect.max.x - 0.5 {
            let cell_w = tile_size.x.min(rect.max.x - x);
            let mut sprite = Sprite::new(
                Vec2::new(x + cell_w * 0.5, y + cell_h * 0.5),
                Vec2::new(cell_w, cell_h),
            )
            .with_z(z)
            .with_color(tint);
            sprite.uv_min = uv_min;
            sprite.uv_max =
                uv_min + Vec2::new(span.x * cell_w / tile_size.x, span.y * cell_h / tile_size.y);
            renderer.draw_sprite(texture, sprite);
            x += tile_size.x;
        }
        y += tile_size.y;
    }
}

/// Draws a ramp as a rotated stone slab with a lit top edge.
fn draw_slope(
    renderer: &mut Renderer,
    stone: TextureHandle,
    uv: (Vec2, Vec2),
    slope: &aurora_engine::Slope,
    tint: Color,
    z: f32,
) {
    let dx = slope.bounds.max.x - slope.bounds.min.x;
    let dy = slope.surface_right - slope.surface_left;
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f32::EPSILON {
        return;
    }
    let angle = dy.atan2(dx);
    let center = Vec2::new(
        (slope.bounds.min.x + slope.bounds.max.x) * 0.5,
        (slope.surface_left + slope.surface_right) * 0.5,
    );
    let mut slab = Sprite::new(center, Vec2::new(length, 16.0))
        .with_rotation(angle)
        .with_color(tint)
        .with_z(z);
    slab.uv_min = uv.0;
    slab.uv_max = uv.1;
    renderer.draw_sprite(stone, slab);
    renderer.draw_sprite(
        stone,
        Sprite::new(center + Vec2::new(0.0, 6.0), Vec2::new(length, 4.0))
            .with_rotation(angle)
            .with_color(Color::rgba(0.62, 0.76, 0.95, 0.45))
            .with_z(z + 0.01),
    );
}

/// Gentle two-voice ambient loop in A-minor pentatonic. Driven by the
/// engine's deterministic note sequencer through the mixer's Music and
/// Ambience channels.
fn ambient_melody() -> Melody {
    Melody::new(
        16.0,
        vec![
            Note::new(0.0, 110.0, 3.4, 0.06),
            Note::new(4.0, 87.31, 3.4, 0.06),
            Note::new(8.0, 130.81, 3.4, 0.06),
            Note::new(12.0, 98.0, 3.4, 0.06),
            Note::new(1.0, 329.63, 0.3, 0.035),
            Note::new(3.0, 440.0, 0.3, 0.035),
            Note::new(5.0, 523.25, 0.3, 0.035),
            Note::new(7.0, 659.26, 0.3, 0.035),
            Note::new(9.0, 523.25, 0.3, 0.035),
            Note::new(11.0, 440.0, 0.3, 0.035),
            Note::new(13.0, 329.63, 0.3, 0.035),
            Note::new(15.0, 293.66, 0.3, 0.035),
        ],
    )
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    env_logger::init();
    run(PlatformerGame::new());
}
