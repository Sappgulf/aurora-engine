//! Aurora Run — collect orbs, dodge hazards. Showcases M2 systems.

use aurora_engine::{
    run, Aabb, Animation, Color, FrameCtx, Game, ParticleSystem, Renderer, RngLite, Sprite,
    Texture, TextureAtlas, TextureHandle, XorShift32,
};
use glam::Vec2;
use winit::keyboard::KeyCode;

const WORLD: Vec2 = Vec2::new(900.0, 520.0);
const PLAYER_SPEED: f32 = 340.0;
const PLAYER_SIZE: f32 = 40.0;

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
    tex_hazard: TextureHandle,
    tex_floor: TextureHandle,
    player_atlas: TextureAtlas,
    player_anim: Animation,
    player: Vec2,
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
            tex_hazard: TextureHandle::default(),
            tex_floor: TextureHandle::default(),
            player_atlas: TextureAtlas::new(TextureHandle::default(), 4, 1, Vec2::new(256.0, 64.0)),
            player_anim: Animation::new([0, 1, 2, 3, 2, 1], 10.0),
            player: Vec2::ZERO,
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
                pos: Vec2::new(a.cos() * 280.0, a.sin() * 160.0),
                alive: true,
            });
        }
        for i in 0..5 {
            let a = i as f32 * 1.3;
            let speed = 80.0 + i as f32 * 25.0;
            self.hazards.push(Hazard {
                pos: Vec2::new(a.cos() * 200.0, a.sin() * 120.0),
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
        let (player_tex, orb, hazard, floor) = {
            let gpu = renderer.gpu();
            (
                Texture::orb_atlas_strip(&gpu, 64, 4, Color::AURORA_TEAL),
                Texture::soft_circle(&gpu, 48, Color::rgb(0.95, 0.85, 0.3)),
                Texture::soft_circle(&gpu, 48, Color::rgb(1.0, 0.25, 0.35)),
                Texture::checker(
                    &gpu,
                    128,
                    16,
                    Color::rgb(0.08, 0.1, 0.16),
                    Color::rgb(0.12, 0.14, 0.22),
                ),
            )
        };
        self.tex_player = renderer.add_texture(player_tex);
        self.tex_orb = renderer.add_texture(orb);
        self.tex_hazard = renderer.add_texture(hazard);
        self.tex_floor = renderer.add_texture(floor);
        self.player_atlas = TextureAtlas::new(self.tex_player, 4, 1, Vec2::new(256.0, 64.0));

        renderer.post_fx.enabled = true;
        renderer.post_fx.bloom_intensity = 1.0;
        renderer.post_fx.vignette = 0.6;
        renderer.post_fx.chromatic = 0.003;
        renderer.set_clear_color(Color::AURORA_NIGHT);
        renderer.camera.zoom = 1.0;

        self.player = Vec2::ZERO;
        self.score = 0;
        self.lives = 3;
        self.collectibles.clear();
        self.hazards.clear();
        for i in 0..12 {
            let a = (i as f32 / 12.0) * std::f32::consts::TAU;
            self.collectibles.push(Collectible {
                pos: Vec2::new(a.cos() * 280.0, a.sin() * 160.0),
                alive: true,
            });
        }
        for i in 0..5 {
            let a = i as f32 * 1.3;
            let speed = 80.0 + i as f32 * 25.0;
            self.hazards.push(Hazard {
                pos: Vec2::new(a.cos() * 200.0, a.sin() * 120.0),
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

        // Move
        let dir = ctx.input.axis_wasd();
        self.player += dir * PLAYER_SPEED * dt;
        let half = WORLD * 0.5 - Vec2::splat(24.0);
        self.player = self.player.clamp(-half, half);

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
        ctx.renderer.camera.position =
            ctx.renderer.camera.position.lerp(self.player, 0.14) + shake_off;

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
            Sprite::new(Vec2::ZERO, WORLD + Vec2::splat(80.0)).with_z(-5.0),
        );

        // Arena border glow
        let border = Color::rgba(0.3, 0.8, 1.0, 0.15);
        let t_orb = self.tex_orb;
        for (pos, size) in [
            (Vec2::new(0.0, WORLD.y * 0.5), Vec2::new(WORLD.x, 8.0)),
            (Vec2::new(0.0, -WORLD.y * 0.5), Vec2::new(WORLD.x, 8.0)),
            (Vec2::new(WORLD.x * 0.5, 0.0), Vec2::new(8.0, WORLD.y)),
            (Vec2::new(-WORLD.x * 0.5, 0.0), Vec2::new(8.0, WORLD.y)),
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
                self.tex_orb,
                Sprite::new(c.pos, Vec2::splat(32.0 * pulse))
                    .with_color(Color::rgba(1.0, 0.92, 0.4, 0.95))
                    .with_z(0.0),
            );
        }

        // Hazards
        for h in &self.hazards {
            let rot = t * 2.0 + h.pos.x * 0.01;
            ctx.renderer.draw_sprite(
                self.tex_hazard,
                Sprite::new(h.pos, Vec2::splat(h.size))
                    .with_rotation(rot)
                    .with_color(Color::rgba(1.0, 0.35, 0.4, 0.9))
                    .with_z(0.2),
            );
        }

        // Particles
        let mut ps = Vec::new();
        self.particles.collect_sprites(&mut ps);
        for s in ps {
            ctx.renderer.draw_sprite(self.tex_orb, s);
        }

        // Player (atlas animation)
        let frame = self.player_anim.frame();
        let flash = if self.hurt_cooldown > 0.0 && ((t * 20.0) as i32 % 2 == 0) {
            Color::rgba(1.0, 0.5, 0.5, 0.9)
        } else {
            Color::WHITE
        };
        let mut spr = self
            .player_atlas
            .sprite(self.player, Vec2::splat(PLAYER_SIZE), frame);
        spr.color = flash;
        spr.z = 1.0;
        ctx.renderer.draw_sprite(self.tex_player, spr);

        // HUD-ish world markers for score/lives (simple dots)
        for i in 0..self.lives.max(0) {
            let p = ctx.renderer.camera.position + Vec2::new(-360.0 + i as f32 * 28.0, 240.0);
            ctx.renderer.draw_sprite(
                self.tex_player,
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
