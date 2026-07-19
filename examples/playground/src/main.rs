//! Aurora Engine M1 playground — sprites, camera, input, particles.

use aurora_engine::{
    run, Color, FrameCtx, Game, ParticleSystem, Renderer, Sprite, Texture, XorShift32,
};
use glam::Vec2;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

struct Playground {
    tex_orb: usize,
    tex_checker: usize,
    tex_player: usize,
    tex_beam: usize,
    player: Vec2,
    orbs: Vec<Orb>,
    particles: ParticleSystem,
    rng: XorShift32,
    burst_cooldown: f32,
}

struct Orb {
    pos: Vec2,
    phase: f32,
    radius: f32,
    hue: f32,
    size: f32,
}

impl Playground {
    fn new() -> Self {
        Self {
            tex_orb: 0,
            tex_checker: 0,
            tex_player: 0,
            tex_beam: 0,
            player: Vec2::ZERO,
            orbs: Vec::new(),
            particles: ParticleSystem::new(2048),
            rng: XorShift32::new(0xA40A_u32),
            burst_cooldown: 0.0,
        }
    }
}

impl Game for Playground {
    fn name(&self) -> &str {
        "Aurora Engine — Playground"
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        let (orb, checker, player, beam) = {
            let gpu = renderer.gpu();
            (
                Texture::soft_circle(&gpu, 64, Color::rgba(1.0, 1.0, 1.0, 1.0)),
                Texture::checker(
                    &gpu,
                    128,
                    16,
                    Color::rgb(0.12, 0.14, 0.22),
                    Color::rgb(0.18, 0.22, 0.35),
                ),
                Texture::soft_circle(&gpu, 48, Color::AURORA_TEAL),
                Texture::gradient_h(
                    &gpu,
                    64,
                    Color::rgba(0.2, 0.9, 0.8, 0.0),
                    Color::rgba(0.7, 0.3, 1.0, 1.0),
                ),
            )
        };
        self.tex_orb = renderer.add_texture(orb);
        self.tex_checker = renderer.add_texture(checker);
        self.tex_player = renderer.add_texture(player);
        self.tex_beam = renderer.add_texture(beam);

        // Floating aurora orbs
        for i in 0..24 {
            let a = (i as f32 / 24.0) * std::f32::consts::TAU;
            self.orbs.push(Orb {
                pos: Vec2::new(a.cos() * 280.0, a.sin() * 160.0),
                phase: i as f32 * 0.4,
                radius: 40.0 + (i % 5) as f32 * 18.0,
                hue: i as f32 / 24.0,
                size: 28.0 + (i % 4) as f32 * 12.0,
            });
        }

        renderer.camera.position = Vec2::ZERO;
        renderer.camera.zoom = 1.0;
        renderer.set_clear_color(Color::AURORA_NIGHT);

        log::info!(
            "Playground ready — WASD move, scroll zoom, click burst, RMB pan, T triangle, R reset"
        );
    }

    fn on_fixed_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let dt = ctx.time.fixed_dt;
        let _ = ctx.audio; // available for SFX
        let speed = 320.0;
        let move_dir = ctx.input.axis_wasd();
        self.player += move_dir * speed * dt;

        // Camera gently follows player
        let target = self.player;
        ctx.renderer.camera.position = ctx.renderer.camera.position.lerp(target, 0.12);

        // RMB pan
        if ctx.input.mouse_down(MouseButton::Right) {
            let inv_zoom = 1.0 / ctx.renderer.camera.zoom;
            let delta = ctx.input.mouse_delta * Vec2::new(-1.0, 1.0) * inv_zoom;
            ctx.renderer.camera.pan(delta);
        }

        self.particles.update(dt);
        self.burst_cooldown = (self.burst_cooldown - dt).max(0.0);
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let t = ctx.time.elapsed;
        let dt = ctx.time.delta;

        // Zoom
        if ctx.input.scroll.abs() > 0.0 {
            let factor = if ctx.input.scroll > 0.0 {
                1.12
            } else {
                1.0 / 1.12
            };
            let screen = ctx.input.mouse_position;
            ctx.renderer.camera.zoom_at(factor, screen);
        }
        if ctx.input.key_down(KeyCode::Equal) || ctx.input.key_down(KeyCode::NumpadAdd) {
            ctx.renderer.camera.zoom_by(1.0 + dt);
        }
        if ctx.input.key_down(KeyCode::Minus) || ctx.input.key_down(KeyCode::NumpadSubtract) {
            ctx.renderer.camera.zoom_by(1.0 - dt);
        }

        // Click / space = particle burst at player or mouse world
        if (ctx.input.mouse_pressed(MouseButton::Left) || ctx.input.key_pressed(KeyCode::Space))
            && self.burst_cooldown <= 0.0
        {
            let world = if ctx.input.mouse_pressed(MouseButton::Left) {
                ctx.renderer
                    .camera
                    .screen_to_world(ctx.input.mouse_position)
            } else {
                self.player
            };
            let color = Color::from_hue((t * 0.15) % 1.0);
            self.particles
                .emit_burst(world, 48, 280.0, 0.9, 22.0, color, &mut self.rng);
            self.burst_cooldown = 0.08;
        }

        // Trail while moving
        if ctx.input.axis_wasd().length_squared() > 0.0 {
            let c = Color::rgba(0.2, 0.95, 0.8, 0.85);
            self.particles.emit_trail(self.player, c, &mut self.rng);
        }

        if ctx.input.key_pressed(KeyCode::KeyT) {
            let on = !ctx.renderer.debug_triangle();
            ctx.renderer.set_debug_triangle(on);
        }
        if ctx.input.key_pressed(KeyCode::KeyR) {
            self.player = Vec2::ZERO;
            ctx.renderer.camera.position = Vec2::ZERO;
            ctx.renderer.camera.zoom = 1.0;
        }

        // Atmosphere clear
        let hue = (t * 0.03) % 1.0;
        ctx.renderer
            .set_clear_color(Color::from_hue(hue).night_blend(0.88));

        // Floor checker (big)
        ctx.renderer.draw_sprite(
            self.tex_checker,
            Sprite::new(Vec2::new(0.0, -40.0), Vec2::new(1600.0, 900.0)).with_z(-2.0),
        );

        // Decorative beams
        for i in 0..6 {
            let a = t * 0.2 + i as f32 * 1.1;
            let pos = Vec2::new(a.cos() * 420.0, a.sin() * 90.0 - 20.0);
            ctx.renderer.draw_sprite(
                self.tex_beam,
                Sprite::new(pos, Vec2::new(220.0, 18.0))
                    .with_rotation(a * 0.5)
                    .with_color(Color::rgba(1.0, 1.0, 1.0, 0.35))
                    .with_z(-1.0),
            );
        }

        // Orbs
        for orb in &mut self.orbs {
            let wobble = (t * 1.3 + orb.phase).sin() * orb.radius;
            let pos = orb.pos + Vec2::new(wobble * 0.15, (t * 0.8 + orb.phase).cos() * 24.0);
            let pulse = 1.0 + 0.12 * (t * 2.5 + orb.phase).sin();
            let mut c = Color::from_hue((orb.hue + t * 0.05) % 1.0);
            c.a = 0.85;
            ctx.renderer.draw_sprite(
                self.tex_orb,
                Sprite::new(pos, Vec2::splat(orb.size * pulse))
                    .with_color(c)
                    .with_z(0.0),
            );
        }

        // Particles
        let mut p_sprites = Vec::with_capacity(self.particles.len());
        self.particles.collect_sprites(&mut p_sprites);
        for s in p_sprites {
            ctx.renderer.draw_sprite(self.tex_orb, s);
        }

        // Player
        let bob = (t * 6.0).sin() * 3.0;
        ctx.renderer.draw_sprite(
            self.tex_player,
            Sprite::new(self.player + Vec2::new(0.0, bob), Vec2::splat(48.0))
                .with_color(Color::rgba(0.3, 1.0, 0.85, 1.0))
                .with_z(1.0),
        );
        // Core
        ctx.renderer.draw_sprite(
            self.tex_orb,
            Sprite::new(self.player + Vec2::new(0.0, bob), Vec2::splat(18.0))
                .with_color(Color::rgba(1.0, 1.0, 1.0, 0.95))
                .with_z(1.1),
        );
    }
}

fn main() {
    run(Playground::new());
}
