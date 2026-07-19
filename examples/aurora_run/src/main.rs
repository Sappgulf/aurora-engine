//! Aurora Run — collect orbs, dodge hazards. Showcases M2 systems.

use aurora_engine::{
    run, Aabb, Action, Animation, BitmapText, Color, FrameCtx, Game, GameFlow, MenuCommand,
    MenuInput, MenuScreen, MenuState, ParticleSystem, PointLight, Renderer, RngLite, Sprite,
    Texture, TextureAtlas, TextureHandle, XorShift32,
};
use glam::Vec2;
use winit::keyboard::KeyCode;

const WORLD: Vec2 = Vec2::new(1180.0, 660.0);
const PLAYER_MAX_SPEED: f32 = 430.0;
const PLAYER_ACCEL_RESPONSE: f32 = 13.0;
const PLAYER_BRAKE_RESPONSE: f32 = 8.5;
const PLAYER_SIZE: f32 = 40.0;
const COMBAT_ATLAS_SIZE: Vec2 = Vec2::new(1774.0, 887.0);

struct Collectible {
    pos: Vec2,
    alive: bool,
    upgrade: bool,
}

#[derive(Clone, Copy)]
enum HazardPattern {
    Bounce,
    Orbit,
    Hunter,
}

struct Hazard {
    pos: Vec2,
    vel: Vec2,
    size: f32,
    pattern: HazardPattern,
    phase: f32,
}

struct AuroraRun {
    tex_player: TextureHandle,
    tex_orb: TextureHandle,
    tex_crystal: TextureHandle,
    tex_hazard: TextureHandle,
    tex_floor: TextureHandle,
    tex_ui: TextureHandle,
    player_atlas: TextureAtlas,
    drone_atlas: TextureAtlas,
    player_anim: Animation,
    player: Vec2,
    player_velocity: Vec2,
    collectibles: Vec<Collectible>,
    hazards: Vec<Hazard>,
    particles: ParticleSystem,
    rng: XorShift32,
    score: u32,
    lives: i32,
    hurt_cooldown: f32,
    shake: f32,
    game_over: bool,
    win: bool,
    spawn_timer: f32,
    wave: u32,
    combo: u32,
    combo_timer: f32,
    dash_timer: f32,
    dash_cooldown: f32,
    dash_charges: u32,
    facing: Vec2,
    menu: MenuState,
}

impl AuroraRun {
    fn new() -> Self {
        Self {
            tex_player: TextureHandle::default(),
            tex_orb: TextureHandle::default(),
            tex_crystal: TextureHandle::default(),
            tex_hazard: TextureHandle::default(),
            tex_floor: TextureHandle::default(),
            tex_ui: TextureHandle::default(),
            player_atlas: TextureAtlas::new(TextureHandle::default(), 4, 2, COMBAT_ATLAS_SIZE),
            drone_atlas: TextureAtlas::new(TextureHandle::default(), 4, 2, COMBAT_ATLAS_SIZE),
            player_anim: Animation::new([0, 1, 2, 3, 2, 1], 10.0),
            player: Vec2::ZERO,
            player_velocity: Vec2::ZERO,
            collectibles: Vec::new(),
            hazards: Vec::new(),
            particles: ParticleSystem::new(1500),
            rng: XorShift32::new(0xC0FFEE),
            score: 0,
            lives: 3,
            hurt_cooldown: 0.0,
            shake: 0.0,
            game_over: false,
            win: false,
            spawn_timer: 0.0,
            wave: 1,
            combo: 0,
            combo_timer: 0.0,
            dash_timer: 0.0,
            dash_cooldown: 0.0,
            dash_charges: 2,
            facing: Vec2::Y,
            menu: MenuState::new(),
        }
    }

    fn begin_wave(&mut self) {
        self.collectibles.clear();
        self.hazards.clear();
        let count = (8 + self.wave * 3).min(22);
        for i in 0..count {
            let a = (i as f32 / count as f32) * std::f32::consts::TAU + self.wave as f32 * 0.23;
            let radius = 175.0 + (i % 4) as f32 * 62.0;
            self.collectibles.push(Collectible {
                pos: Vec2::new(a.cos() * radius, a.sin() * (120.0 + (i % 3) as f32 * 56.0)),
                alive: true,
                upgrade: i == count - 1,
            });
        }
        let hazard_count = (3 + self.wave * 2).min(16);
        for i in 0..hazard_count {
            let a = i as f32 * 1.71 + self.wave as f32 * 0.38;
            let speed = 86.0 + self.wave as f32 * 14.0 + (i % 3) as f32 * 22.0;
            self.hazards.push(Hazard {
                pos: Vec2::new(a.cos() * 390.0, a.sin() * 225.0),
                vel: Vec2::new(a.sin(), -a.cos()) * speed,
                size: 25.0 + (i % 3) as f32 * 7.0,
                pattern: match i % 3 {
                    0 => HazardPattern::Bounce,
                    1 => HazardPattern::Orbit,
                    _ => HazardPattern::Hunter,
                },
                phase: a,
            });
        }
    }

    fn reset(&mut self, audio_start: bool, audio: &aurora_engine::Audio) {
        self.player = Vec2::ZERO;
        self.player_velocity = Vec2::ZERO;
        self.score = 0;
        self.lives = 3;
        self.hurt_cooldown = 0.0;
        self.shake = 0.0;
        self.game_over = false;
        self.win = false;
        self.spawn_timer = 0.0;
        self.wave = 1;
        self.combo = 0;
        self.combo_timer = 0.0;
        self.dash_timer = 0.0;
        self.dash_cooldown = 0.0;
        self.dash_charges = 2;
        self.facing = Vec2::Y;
        self.particles = ParticleSystem::new(1500);
        self.begin_wave();
        if audio_start {
            audio.start();
        }
    }

    fn player_aabb(&self) -> Aabb {
        Aabb::from_center_size(self.player, Vec2::splat(PLAYER_SIZE * 0.7))
    }

    fn menu_input(ctx: &FrameCtx<'_>) -> Option<MenuInput> {
        if ctx.input.action_pressed(Action::MenuUp) {
            Some(MenuInput::Up)
        } else if ctx.input.action_pressed(Action::MenuDown) {
            Some(MenuInput::Down)
        } else if ctx.input.action_pressed(Action::MenuConfirm) {
            Some(MenuInput::Confirm)
        } else if ctx.input.action_pressed(Action::MenuBack) {
            Some(MenuInput::Back)
        } else {
            None
        }
    }

    fn handle_menu_command(&mut self, command: MenuCommand, ctx: &mut FrameCtx<'_>) {
        match command {
            MenuCommand::StartRun | MenuCommand::RestartRun => self.reset(true, ctx.audio),
            MenuCommand::Resume => self.shake = 0.05,
            MenuCommand::TogglePostFx => ctx.renderer.post_fx.enabled = self.menu.post_fx,
            MenuCommand::ToggleReducedMotion => {
                if self.menu.reduced_motion {
                    self.shake = 0.0;
                }
            }
            MenuCommand::EndRun | MenuCommand::ReturnToMain => {
                self.game_over = false;
                self.win = false;
                self.player_velocity = Vec2::ZERO;
            }
            MenuCommand::None | MenuCommand::Open(_) => {}
        }
    }

    fn draw_text(
        &self,
        renderer: &mut Renderer,
        text: &str,
        origin: Vec2,
        pixel_size: f32,
        color: Color,
        z: f32,
    ) {
        for cell in BitmapText::glyphs(text, origin, pixel_size) {
            renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(cell.position, Vec2::splat(cell.size))
                    .with_color(color)
                    .with_z(z),
            );
        }
    }

    fn draw_menu(&self, ctx: &mut FrameCtx<'_>, screen: MenuScreen, t: f32) {
        let camera = ctx.renderer.camera.position;
        ctx.renderer.draw_sprite(
            self.tex_ui,
            Sprite::new(camera, Vec2::new(840.0, 560.0))
                .with_color(Color::rgba(0.01, 0.025, 0.08, 0.84))
                .with_z(10.0),
        );
        ctx.renderer.draw_sprite(
            self.tex_ui,
            Sprite::new(camera + Vec2::new(0.0, 224.0), Vec2::new(720.0, 4.0))
                .with_color(Color::rgba(0.1, 1.4, 1.15, 0.7))
                .with_z(10.1),
        );
        let title = match screen {
            MenuScreen::Main => "AURORA RUN",
            MenuScreen::HowTo => "HOW TO PLAY",
            MenuScreen::Settings => "SETTINGS",
            MenuScreen::Pause => "PAUSED",
            MenuScreen::Results if self.win => "RUN COMPLETE",
            MenuScreen::Results => "SYSTEM FAILURE",
        };
        let title_pixel = if title.len() > 10 { 8.0 } else { 11.0 };
        self.draw_text(
            ctx.renderer,
            title,
            camera + Vec2::new(-260.0, 170.0),
            title_pixel,
            Color::rgba(0.25, 1.8, 1.5, 0.95 + 0.05 * (t * 2.0).sin()),
            11.0,
        );

        let items: &[&str] = match screen {
            MenuScreen::Main => &["START RUN", "HOW TO PLAY", "SETTINGS"],
            MenuScreen::HowTo => &["WASD MOVE  SPACE DASH", "COLLECT CRYSTALS", "ESC BACK"],
            MenuScreen::Settings => &[
                if self.menu.post_fx {
                    "POST FX ON"
                } else {
                    "POST FX OFF"
                },
                if self.menu.reduced_motion {
                    "MOTION LOW"
                } else {
                    "MOTION FULL"
                },
                "BACK TO MENU",
            ],
            MenuScreen::Pause => &["RESUME", "RESTART RUN", "SETTINGS", "END RUN"],
            MenuScreen::Results => &["RUN AGAIN", "MAIN MENU"],
        };
        for (index, item) in items.iter().enumerate() {
            let selected = index == self.menu.selected() && !matches!(screen, MenuScreen::HowTo);
            let y = 78.0 - index as f32 * 57.0;
            if selected {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(camera + Vec2::new(0.0, y + 5.0), Vec2::new(520.0, 38.0))
                        .with_color(Color::rgba(0.05, 0.85, 0.72, 0.18))
                        .with_z(10.2),
                );
            }
            let label = if selected {
                format!("> {}", item)
            } else {
                (*item).to_string()
            };
            self.draw_text(
                ctx.renderer,
                &label,
                camera + Vec2::new(-235.0, y),
                6.0,
                if selected {
                    Color::rgb(0.9, 1.7, 1.35)
                } else {
                    Color::rgba(0.62, 0.78, 0.9, 0.9)
                },
                11.0,
            );
        }
        if matches!(screen, MenuScreen::Results) {
            self.draw_text(
                ctx.renderer,
                &format!("SCORE {}  WAVE {}", self.score, self.wave),
                camera + Vec2::new(-235.0, -140.0),
                5.0,
                Color::rgb(1.5, 0.85, 0.25),
                11.0,
            );
        }
        self.draw_text(
            ctx.renderer,
            "ARROWS OR WASD  ENTER SELECT  ESC BACK",
            camera + Vec2::new(-300.0, -220.0),
            3.5,
            Color::rgba(0.35, 0.65, 0.82, 0.75),
            11.0,
        );
    }
}

impl Game for AuroraRun {
    fn name(&self) -> &str {
        "Aurora Run — Collect & Dodge"
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        let (player_tex, orb, crystal, floor, ui) = {
            let gpu = renderer.gpu();
            (
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/aurora-combat-atlas.png"),
                    "Aurora combat atlas",
                )
                .expect("generated combat atlas must decode"),
                Texture::soft_circle(&gpu, 48, Color::rgb(0.95, 0.85, 0.3)),
                Texture::crystal(&gpu, 64, Color::rgb(1.0, 0.72, 0.16)),
                Texture::arena_floor(&gpu, 512),
                Texture::solid(&gpu, Color::WHITE),
            )
        };
        self.tex_player = renderer.add_texture(player_tex);
        self.tex_orb = renderer.add_texture(orb);
        self.tex_crystal = renderer.add_texture(crystal);
        self.tex_hazard = self.tex_player;
        self.tex_floor = renderer.add_texture(floor);
        self.tex_ui = renderer.add_texture(ui);
        self.player_atlas = TextureAtlas::new(self.tex_player, 4, 2, COMBAT_ATLAS_SIZE);
        self.drone_atlas = TextureAtlas::new(self.tex_player, 4, 2, COMBAT_ATLAS_SIZE);

        renderer.post_fx.enabled = true;
        renderer.post_fx.bloom_intensity = 1.0;
        renderer.post_fx.vignette = 0.6;
        renderer.post_fx.chromatic = 0.003;
        renderer.set_clear_color(Color::AURORA_NIGHT);
        renderer.camera.zoom = 1.34;

        self.player = Vec2::ZERO;
        self.player_velocity = Vec2::ZERO;
        self.score = 0;
        self.lives = 3;
        self.begin_wave();
        log::info!("Aurora Run — title menu ready. Arrows/WASD navigate, Enter/Space select.");
    }

    fn on_fixed_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let dt = ctx.time.fixed_dt;

        if let Some(input) = Self::menu_input(ctx) {
            let command = self.menu.handle(input);
            self.handle_menu_command(command, ctx);
            return;
        }

        // Menus are modal: keep their arena backdrop alive in `on_update`,
        // but freeze all simulation (movement, hazards, timers, and scoring).
        if self.menu.screen().is_some() {
            return;
        }

        if ctx.input.key_pressed(KeyCode::KeyR) {
            self.reset(true, ctx.audio);
            return;
        }
        if ctx.input.key_pressed(KeyCode::KeyP) {
            ctx.renderer.post_fx.enabled = !ctx.renderer.post_fx.enabled;
        }
        if self.game_over || self.win {
            self.menu.open(MenuScreen::Results);
            return;
        }

        // Accelerated steering preserves the fixed-step collision model while
        // adding a short, controllable sense of mass to each direction change.
        let dir = ctx.input.axis_wasd();
        if dir.length_squared() > 0.0 {
            self.facing = dir.normalize();
        }
        self.dash_timer = (self.dash_timer - dt).max(0.0);
        self.dash_cooldown = (self.dash_cooldown - dt).max(0.0);
        if ctx.input.key_pressed(KeyCode::Space)
            && self.dash_charges > 0
            && self.dash_cooldown <= 0.0
        {
            self.dash_charges -= 1;
            self.dash_timer = 0.16;
            self.dash_cooldown = 0.48;
            self.hurt_cooldown = self.hurt_cooldown.max(0.25);
            self.player_velocity = self.facing * 980.0;
            self.shake = 0.15;
            self.particles.emit_burst(
                self.player,
                34,
                350.0,
                0.45,
                14.0,
                Color::AURORA_TEAL,
                &mut self.rng,
            );
        }
        let target_velocity = if self.dash_timer > 0.0 {
            self.facing * 940.0
        } else {
            dir * PLAYER_MAX_SPEED
        };
        let response = if dir.length_squared() > 0.0 {
            PLAYER_ACCEL_RESPONSE
        } else {
            PLAYER_BRAKE_RESPONSE
        };
        let steering = 1.0 - (-response * dt).exp();
        self.player_velocity = self.player_velocity.lerp(target_velocity, steering);
        self.player += self.player_velocity * dt;
        let half = WORLD * 0.5 - Vec2::splat(24.0);
        let clamped = self.player.clamp(-half, half);
        if clamped.x != self.player.x {
            self.player_velocity.x = 0.0;
        }
        if clamped.y != self.player.y {
            self.player_velocity.y = 0.0;
        }
        self.player = clamped;

        if dir.length_squared() > 0.0 {
            self.player_anim.tick(dt);
        }

        // Hazards bounce in world
        let bound = WORLD * 0.5;
        for h in &mut self.hazards {
            h.phase += dt;
            match h.pattern {
                HazardPattern::Bounce => {}
                HazardPattern::Orbit => {
                    let radial = h.pos.normalize_or_zero();
                    h.vel = Vec2::new(-radial.y, radial.x) * (105.0 + self.wave as f32 * 13.0)
                        + radial * (h.phase * 2.0).sin() * 28.0;
                }
                HazardPattern::Hunter => {
                    let seek = (self.player - h.pos).normalize_or_zero()
                        * (80.0 + self.wave as f32 * 10.0);
                    h.vel = h.vel.lerp(seek, 1.0 - (-dt * 1.45).exp());
                }
            }
            h.pos += h.vel * dt;
            if h.pos.x < -bound.x || h.pos.x > bound.x {
                h.vel.x *= -1.0;
                h.pos.x = h.pos.x.clamp(-bound.x, bound.x);
            }
            if h.pos.y < -bound.y || h.pos.y > bound.y {
                h.vel.y *= -1.0;
                h.pos.y = h.pos.y.clamp(-bound.y, bound.y);
            }
        }

        // Collect
        let p_box = self.player_aabb();
        for c in &mut self.collectibles {
            if !c.alive {
                continue;
            }
            let box_c = Aabb::from_center_size(c.pos, Vec2::splat(28.0));
            if p_box.intersects(box_c) {
                c.alive = false;
                self.combo = self.combo.saturating_add(1);
                self.combo_timer = 2.4;
                self.score += if c.upgrade { 6 } else { 1 + self.combo / 4 };
                if c.upgrade {
                    self.dash_charges = (self.dash_charges + 1).min(3);
                    self.lives = (self.lives + 1).min(5);
                    self.particles.emit_burst(
                        c.pos,
                        52,
                        300.0,
                        0.85,
                        23.0,
                        Color::rgb(0.25, 1.0, 0.72),
                        &mut self.rng,
                    );
                }
                ctx.audio.collect();
                self.particles.emit_burst(
                    c.pos,
                    24,
                    200.0,
                    0.6,
                    16.0,
                    Color::rgb(1.0, 0.9, 0.3),
                    &mut self.rng,
                );
            }
        }

        if self.collectibles.iter().all(|c| !c.alive) {
            self.wave += 1;
            self.dash_charges = (self.dash_charges + 1).min(3);
            self.combo_timer = 3.0;
            ctx.audio.win_note();
            self.particles.emit_burst(
                self.player,
                60,
                320.0,
                1.0,
                28.0,
                Color::AURORA_TEAL,
                &mut self.rng,
            );
            if self.wave > 6 {
                self.win = true;
                self.menu.open(MenuScreen::Results);
            } else {
                self.begin_wave();
            }
        }

        // Hazards damage
        self.hurt_cooldown = (self.hurt_cooldown - dt).max(0.0);
        if self.hurt_cooldown <= 0.0 {
            for h in &self.hazards {
                let box_h = Aabb::from_center_size(h.pos, Vec2::splat(h.size * 0.75));
                if p_box.intersects(box_h) {
                    self.lives -= 1;
                    self.hurt_cooldown = 1.0;
                    self.combo = 0;
                    self.combo_timer = 0.0;
                    self.shake = 0.45;
                    ctx.audio.hurt();
                    ctx.renderer.post_fx.chromatic = 0.018;
                    self.particles.emit_burst(
                        self.player,
                        30,
                        250.0,
                        0.5,
                        18.0,
                        Color::rgb(1.0, 0.3, 0.4),
                        &mut self.rng,
                    );
                    if self.lives <= 0 {
                        self.game_over = true;
                        self.menu.open(MenuScreen::Results);
                    }
                    break;
                }
            }
        }

        // Pressure ramps inside a wave, then resets when its last crystal is claimed.
        self.spawn_timer += dt;
        if self.spawn_timer > 7.0 && self.hazards.len() < 18 {
            self.spawn_timer = 0.0;
            let a = self.rng.f32() * std::f32::consts::TAU;
            let speed = 105.0 + self.rng.f32() * 80.0 + self.wave as f32 * 12.0;
            self.hazards.push(Hazard {
                pos: Vec2::new(a.cos() * 300.0, a.sin() * 180.0),
                vel: Vec2::new(a.sin(), -a.cos()) * speed,
                size: 26.0 + self.rng.f32() * 16.0,
                pattern: if self.rng.f32() > 0.5 {
                    HazardPattern::Orbit
                } else {
                    HazardPattern::Hunter
                },
                phase: a,
            });
        }

        self.particles.update(dt);
        self.combo_timer = (self.combo_timer - dt).max(0.0);
        if self.combo_timer <= 0.0 {
            self.combo = 0;
        }
        self.shake = (self.shake - dt).max(0.0);

        // Camera follow + shake
        let shake_off = if self.shake > 0.0 {
            Vec2::new(self.rng.f32() - 0.5, self.rng.f32() - 0.5) * self.shake * 28.0
        } else {
            Vec2::ZERO
        };
        let camera_target = self.player + self.player_velocity * 0.12;
        let camera_response = 1.0 - (-dt * 7.5).exp();
        ctx.renderer.camera.position = ctx
            .renderer
            .camera
            .position
            .lerp(camera_target, camera_response);
        ctx.renderer.camera.position += shake_off;
        let speed_ratio = (self.player_velocity.length() / PLAYER_MAX_SPEED).clamp(0.0, 1.0);
        let target_zoom = 1.34 - speed_ratio * 0.08;
        ctx.renderer.camera.zoom +=
            (target_zoom - ctx.renderer.camera.zoom) * (1.0 - (-dt * 4.0).exp());

        // Recover chromatic
        let target_ca = 0.003;
        ctx.renderer.post_fx.chromatic +=
            (target_ca - ctx.renderer.post_fx.chromatic) * (1.0 - (-dt * 3.0).exp());
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let t = ctx.time.elapsed;

        // Clear tint
        let clear = if self.game_over {
            Color::rgb(0.12, 0.02, 0.04)
        } else if self.win {
            Color::rgb(0.02, 0.1, 0.1)
        } else {
            Color::from_hue((t * 0.02) % 1.0).night_blend(0.9)
        };
        ctx.renderer.set_clear_color(clear);

        // The arena is camera-derived rather than a fixed pixel rectangle, so
        // it fills native windows, browser canvases, and DPI-scaled displays.
        let view_size = ctx.renderer.camera.visible_world_size();
        let arena_center = ctx.renderer.camera.position;
        let arena_size = view_size * 1.035;

        // Floor
        ctx.renderer.draw_sprite(
            self.tex_floor,
            Sprite::new(arena_center, arena_size).with_z(-5.0),
        );

        // Soft emissive pools make the player and hazards read as lights while
        // leaving the silhouette and collision geometry crisp.
        ctx.renderer.draw_sprite(
            self.tex_orb,
            Sprite::new(self.player, Vec2::splat(210.0))
                .with_color(Color::rgba(0.15, 1.8, 1.45, 0.14 + 0.03 * (t * 3.0).sin()))
                .with_z(-3.0),
        );
        ctx.renderer.draw_light(PointLight::new(
            self.player,
            Color::rgb(0.08, 1.2, 0.95),
            145.0,
            0.46 + 0.08 * (t * 3.0).sin(),
        ));

        // Arena border glow
        let border = Color::rgba(0.08, 1.6, 1.35, 0.28);
        let t_orb = self.tex_orb;
        for (pos, size) in [
            (
                arena_center + Vec2::new(0.0, arena_size.y * 0.5),
                Vec2::new(arena_size.x, 14.0),
            ),
            (
                arena_center - Vec2::new(0.0, arena_size.y * 0.5),
                Vec2::new(arena_size.x, 14.0),
            ),
            (
                arena_center + Vec2::new(arena_size.x * 0.5, 0.0),
                Vec2::new(14.0, arena_size.y),
            ),
            (
                arena_center - Vec2::new(arena_size.x * 0.5, 0.0),
                Vec2::new(14.0, arena_size.y),
            ),
        ] {
            ctx.renderer.draw_sprite(
                t_orb,
                Sprite::new(pos, size).with_color(border).with_z(-4.0),
            );
        }

        // Collectibles
        for c in &self.collectibles {
            if !c.alive {
                continue;
            }
            let pulse = 1.0 + 0.15 * (t * 4.0 + c.pos.x * 0.01).sin();
            let crystal_color = if c.upgrade {
                Color::rgba(0.3, 2.1, 1.35, 1.0)
            } else {
                Color::rgba(1.8, 1.25, 0.2, 1.0)
            };
            if c.upgrade {
                ctx.renderer.draw_sprite(
                    self.tex_orb,
                    Sprite::new(c.pos, Vec2::splat(90.0 * pulse))
                        .with_color(Color::rgba(0.08, 1.6, 1.0, 0.15))
                        .with_z(-1.5),
                );
            }
            ctx.renderer.draw_sprite(
                self.tex_crystal,
                Sprite::new(c.pos, Vec2::splat(38.0 * pulse))
                    .with_color(crystal_color)
                    .with_z(0.0),
            );
        }

        // Hazards
        for h in &self.hazards {
            let rot = t * 2.0 + h.pos.x * 0.01;
            ctx.renderer.draw_light(PointLight::new(
                h.pos,
                Color::rgb(1.4, 0.03, 0.12),
                h.size * 2.7,
                0.24,
            ));
            let heading = h.vel.normalize_or_zero();
            let heading_rotation = heading.y.atan2(heading.x);
            for trail in 1..=3 {
                let amount = trail as f32;
                ctx.renderer.draw_sprite(
                    self.tex_orb,
                    Sprite::new(
                        h.pos - heading * h.size * amount * 0.72,
                        Vec2::new(
                            h.size * (4.0 - amount * 0.65),
                            h.size * (1.7 - amount * 0.22),
                        ),
                    )
                    .with_rotation(heading_rotation)
                    .with_color(Color::rgba(2.2, 0.05, 0.16, 0.11 - amount * 0.018))
                    .with_z(-2.5),
                );
            }
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(h.pos, Vec2::splat(h.size * 5.0))
                    .with_color(Color::rgba(2.4, 0.08, 0.18, 0.10))
                    .with_z(-2.0),
            );
            let mut drone = self.drone_atlas.sprite(
                h.pos,
                Vec2::splat(h.size * 2.9),
                4 + ((t * 3.0 + h.pos.x * 0.01).abs() as u32 % 4),
            );
            drone.rotation = rot * 0.12;
            drone.color = match h.pattern {
                HazardPattern::Bounce => Color::rgba(1.3, 0.42, 0.42, 1.0),
                HazardPattern::Orbit => Color::rgba(1.15, 0.35, 0.9, 1.0),
                HazardPattern::Hunter => Color::rgba(1.65, 0.22, 0.25, 1.0),
            };
            drone.z = 0.2;
            ctx.renderer.draw_sprite(self.tex_hazard, drone);
        }

        // Particles
        let mut ps = Vec::new();
        self.particles.collect_sprites(&mut ps);
        for s in ps {
            ctx.renderer.draw_sprite(self.tex_orb, s);
        }

        // Player (atlas animation)
        let frame = self.player_anim.frame() % 4;
        let flash = if self.hurt_cooldown > 0.0 && ((t * 20.0) as i32 % 2 == 0) {
            Color::rgba(1.0, 0.5, 0.5, 0.9)
        } else {
            Color::WHITE
        };
        let speed_ratio = (self.player_velocity.length() / PLAYER_MAX_SPEED).clamp(0.0, 1.0);
        if self.dash_timer > 0.0 {
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(self.player, Vec2::splat(155.0 + self.dash_timer * 280.0))
                    .with_color(Color::rgba(0.15, 2.4, 2.0, 0.25))
                    .with_z(-0.5),
            );
        }
        if speed_ratio > 0.06 {
            let heading = self.player_velocity.normalize();
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(
                    self.player - heading * (34.0 + speed_ratio * 20.0),
                    Vec2::new(100.0 + speed_ratio * 90.0, 28.0),
                )
                .with_rotation(heading.y.atan2(heading.x))
                .with_color(Color::rgba(0.1, 1.4, 1.3, 0.14 + speed_ratio * 0.13))
                .with_z(-1.0),
            );
        }
        let mut spr = self
            .player_atlas
            .sprite(self.player, Vec2::splat(PLAYER_SIZE * 2.45), frame);
        spr.color = flash;
        spr.rotation = -self.player_velocity.x * 0.00055;
        spr.z = 1.0;
        ctx.renderer.draw_sprite(self.tex_player, spr);

        // Compact viewport-anchored HUD: life/dash left, score/combo right,
        // and wave progress top-center. These anchors stay at the screen edge
        // as the camera follows the player or the window changes size.
        let hud_top_left = ctx
            .renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 1.0))
            + Vec2::new(42.0, -42.0);
        let hud_top_right = ctx
            .renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(1.0, 1.0))
            + Vec2::new(-42.0, -42.0);
        let hud_top_center = ctx
            .renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.5, 1.0))
            + Vec2::new(0.0, -42.0);
        for i in 0..self.dash_charges {
            let p = hud_top_left + Vec2::new(i as f32 * 22.0, -28.0);
            ctx.renderer.draw_sprite(
                self.tex_crystal,
                Sprite::new(p, Vec2::splat(16.0))
                    .with_color(Color::rgba(0.1, 1.9, 1.6, 1.0))
                    .with_z(5.0),
            );
        }
        for i in 0..self.lives.max(0) {
            let p = hud_top_left + Vec2::new(i as f32 * 28.0, 0.0);
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(p, Vec2::splat(18.0))
                    .with_color(Color::AURORA_TEAL)
                    .with_z(5.0),
            );
        }
        for i in 0..self.score.min(20) {
            let p = hud_top_right + Vec2::new(-((i % 10) as f32) * 16.0, -((i / 10) as f32) * 16.0);
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(p, Vec2::splat(12.0))
                    .with_color(Color::rgb(1.0, 0.85, 0.3))
                    .with_z(5.0),
            );
        }

        let alive = self.collectibles.iter().filter(|c| c.alive).count() as f32;
        let total = self.collectibles.len().max(1) as f32;
        let progress = 1.0 - alive / total;
        ctx.renderer.draw_sprite(
            self.tex_orb,
            Sprite::new(hud_top_center, Vec2::new(220.0, 9.0))
                .with_color(Color::rgba(0.01, 0.08, 0.12, 0.85))
                .with_z(5.0),
        );
        ctx.renderer.draw_sprite(
            self.tex_orb,
            Sprite::new(
                hud_top_center + Vec2::new(-110.0 + progress * 110.0, 0.0),
                Vec2::new(220.0 * progress, 7.0),
            )
            .with_color(Color::rgba(1.6, 0.85, 0.18, 0.9))
            .with_z(5.1),
        );
        for i in 0..self.wave.min(6) {
            let p = hud_top_center + Vec2::new(-34.0 + i as f32 * 14.0, -23.0);
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(p, Vec2::splat(8.0))
                    .with_color(Color::rgba(1.7, 0.18, 0.38, 0.9))
                    .with_z(5.0),
            );
        }
        if self.combo > 1 {
            for i in 0..self.combo.min(12) {
                let p = hud_top_right + Vec2::new(-(i as f32) * 11.0, -28.0);
                ctx.renderer.draw_sprite(
                    self.tex_crystal,
                    Sprite::new(p, Vec2::splat(10.0 + (t * 5.0).sin().max(0.0) * 3.0))
                        .with_color(Color::rgba(1.9, 0.72, 0.15, 1.0))
                        .with_z(5.0),
                );
            }
        }

        // Menus are a separate game-flow layer over the live arena. They stop
        // simulation in fixed update while retaining the scene as atmosphere.
        if let GameFlow::Menu(screen) = self.menu.flow {
            self.draw_menu(ctx, screen, t);
        }
    }
}

fn main() {
    run(AuroraRun::new());
}
