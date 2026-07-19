//! Aurora Run — collect orbs, dodge hazards. Showcases M2 systems.

use aurora_engine::{
    run, Aabb, Animation, Color, FrameCtx, Game, ParticleSystem, Renderer, RngLite, Sprite,
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
}

struct Hazard {
    pos: Vec2,
    vel: Vec2,
    size: f32,
}

struct AuroraRun {
    tex_player: TextureHandle,
    tex_orb: TextureHandle,
    tex_crystal: TextureHandle,
    tex_hazard: TextureHandle,
    tex_floor: TextureHandle,
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
}

impl AuroraRun {
    fn new() -> Self {
        Self {
            tex_player: TextureHandle::default(),
            tex_orb: TextureHandle::default(),
            tex_crystal: TextureHandle::default(),
            tex_hazard: TextureHandle::default(),
            tex_floor: TextureHandle::default(),
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
        self.particles = ParticleSystem::new(1500);
        self.collectibles.clear();
        self.hazards.clear();
        for i in 0..12 {
            let a = (i as f32 / 12.0) * std::f32::consts::TAU;
            self.collectibles.push(Collectible {
                pos: Vec2::new(a.cos() * 400.0, a.sin() * 235.0),
                alive: true,
            });
        }
        for i in 0..5 {
            let a = i as f32 * 1.3;
            let speed = 80.0 + i as f32 * 25.0;
            self.hazards.push(Hazard {
                pos: Vec2::new(a.cos() * 350.0, a.sin() * 205.0),
                vel: Vec2::new(a.sin(), -a.cos()) * speed,
                size: 28.0 + (i % 3) as f32 * 8.0,
            });
        }
        if audio_start {
            audio.start();
        }
    }

    fn player_aabb(&self) -> Aabb {
        Aabb::from_center_size(self.player, Vec2::splat(PLAYER_SIZE * 0.7))
    }
}

impl Game for AuroraRun {
    fn name(&self) -> &str {
        "Aurora Run — Collect & Dodge"
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        let (player_tex, orb, crystal, floor) = {
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
            )
        };
        self.tex_player = renderer.add_texture(player_tex);
        self.tex_orb = renderer.add_texture(orb);
        self.tex_crystal = renderer.add_texture(crystal);
        self.tex_hazard = self.tex_player;
        self.tex_floor = renderer.add_texture(floor);
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
        self.collectibles.clear();
        self.hazards.clear();
        for i in 0..12 {
            let a = (i as f32 / 12.0) * std::f32::consts::TAU;
            self.collectibles.push(Collectible {
                pos: Vec2::new(a.cos() * 400.0, a.sin() * 235.0),
                alive: true,
            });
        }
        for i in 0..5 {
            let a = i as f32 * 1.3;
            let speed = 80.0 + i as f32 * 25.0;
            self.hazards.push(Hazard {
                pos: Vec2::new(a.cos() * 350.0, a.sin() * 205.0),
                vel: Vec2::new(a.sin(), -a.cos()) * speed,
                size: 28.0 + (i % 3) as f32 * 8.0,
            });
        }
        log::info!(
            "Aurora Run — WASD move, collect gold orbs, avoid red. P toggles post-FX. R restart."
        );
    }

    fn on_fixed_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let dt = ctx.time.fixed_dt;

        if ctx.input.key_pressed(KeyCode::KeyR) {
            self.reset(true, ctx.audio);
            return;
        }
        if ctx.input.key_pressed(KeyCode::KeyP) {
            ctx.renderer.post_fx.enabled = !ctx.renderer.post_fx.enabled;
        }

        if self.game_over || self.win {
            return;
        }

        // Accelerated steering preserves the fixed-step collision model while
        // adding a short, controllable sense of mass to each direction change.
        let dir = ctx.input.axis_wasd();
        let target_velocity = dir * PLAYER_MAX_SPEED;
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
                self.score += 1;
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

        if self.collectibles.iter().all(|c| !c.alive) && !self.win {
            self.win = true;
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
        }

        // Hazards damage
        self.hurt_cooldown = (self.hurt_cooldown - dt).max(0.0);
        if self.hurt_cooldown <= 0.0 {
            for h in &self.hazards {
                let box_h = Aabb::from_center_size(h.pos, Vec2::splat(h.size * 0.75));
                if p_box.intersects(box_h) {
                    self.lives -= 1;
                    self.hurt_cooldown = 1.0;
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
                    }
                    break;
                }
            }
        }

        // Spawn extra hazards over time
        self.spawn_timer += dt;
        if self.spawn_timer > 8.0 && self.hazards.len() < 12 {
            self.spawn_timer = 0.0;
            let a = self.rng.f32() * std::f32::consts::TAU;
            let speed = 100.0 + self.rng.f32() * 80.0;
            self.hazards.push(Hazard {
                pos: Vec2::new(a.cos() * 300.0, a.sin() * 180.0),
                vel: Vec2::new(a.sin(), -a.cos()) * speed,
                size: 26.0 + self.rng.f32() * 16.0,
            });
        }

        self.particles.update(dt);
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

        // Floor
        ctx.renderer.draw_sprite(
            self.tex_floor,
            Sprite::new(Vec2::ZERO, WORLD + Vec2::splat(100.0)).with_z(-5.0),
        );

        // Soft emissive pools make the player and hazards read as lights while
        // leaving the silhouette and collision geometry crisp.
        ctx.renderer.draw_sprite(
            self.tex_orb,
            Sprite::new(self.player, Vec2::splat(210.0))
                .with_color(Color::rgba(0.15, 1.8, 1.45, 0.14 + 0.03 * (t * 3.0).sin()))
                .with_z(-3.0),
        );

        // Arena border glow
        let border = Color::rgba(0.08, 1.6, 1.35, 0.28);
        let t_orb = self.tex_orb;
        for (pos, size) in [
            (Vec2::new(0.0, WORLD.y * 0.5), Vec2::new(WORLD.x, 14.0)),
            (Vec2::new(0.0, -WORLD.y * 0.5), Vec2::new(WORLD.x, 14.0)),
            (Vec2::new(WORLD.x * 0.5, 0.0), Vec2::new(14.0, WORLD.y)),
            (Vec2::new(-WORLD.x * 0.5, 0.0), Vec2::new(14.0, WORLD.y)),
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
            ctx.renderer.draw_sprite(
                self.tex_crystal,
                Sprite::new(c.pos, Vec2::splat(38.0 * pulse))
                    .with_color(Color::rgba(1.8, 1.25, 0.2, 1.0))
                    .with_z(0.0),
            );
        }

        // Hazards
        for h in &self.hazards {
            let rot = t * 2.0 + h.pos.x * 0.01;
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
            drone.color = Color::rgba(1.3, 0.42, 0.42, 1.0);
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

        // HUD-ish world markers for score/lives (simple dots)
        for i in 0..self.lives.max(0) {
            let p = ctx.renderer.camera.position + Vec2::new(-360.0 + i as f32 * 28.0, 240.0);
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(p, Vec2::splat(18.0))
                    .with_color(Color::AURORA_TEAL)
                    .with_z(5.0),
            );
        }
        for i in 0..self.score.min(20) {
            let p = ctx.renderer.camera.position
                + Vec2::new(
                    300.0 - (i % 10) as f32 * 16.0,
                    240.0 - (i / 10) as f32 * 16.0,
                );
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(p, Vec2::splat(12.0))
                    .with_color(Color::rgb(1.0, 0.85, 0.3))
                    .with_z(5.0),
            );
        }

        // Win / lose overlay pulses
        if self.game_over || self.win {
            let c = if self.win {
                Color::rgba(0.2, 1.0, 0.8, 0.25 + 0.1 * (t * 3.0).sin())
            } else {
                Color::rgba(1.0, 0.1, 0.2, 0.2 + 0.1 * (t * 4.0).sin())
            };
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(ctx.renderer.camera.position, Vec2::new(1400.0, 900.0))
                    .with_color(c)
                    .with_z(10.0),
            );
        }
    }
}

fn main() {
    run(AuroraRun::new());
}
