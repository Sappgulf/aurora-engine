//! Lightweight CPU particle system rendered as sprites.

use glam::Vec2;

use crate::color::Color;
use crate::sprite::Sprite;

#[derive(Debug, Clone)]
struct Particle {
    pos: Vec2,
    vel: Vec2,
    life: f32,
    max_life: f32,
    size: f32,
    color: Color,
    spin: f32,
    angle: f32,
}

/// Simple additive-looking particle emitter (alpha-faded sprites).
#[derive(Debug)]
pub struct ParticleSystem {
    particles: Vec<Particle>,
    capacity: usize,
}

impl ParticleSystem {
    pub fn new(capacity: usize) -> Self {
        Self {
            particles: Vec::with_capacity(capacity),
            capacity,
        }
    }

    pub fn len(&self) -> usize {
        self.particles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    pub fn emit_burst(
        &mut self,
        origin: Vec2,
        count: usize,
        speed: f32,
        life: f32,
        size: f32,
        color: Color,
        rng: &mut impl RngLite,
    ) {
        for _ in 0..count {
            if self.particles.len() >= self.capacity {
                break;
            }
            let angle = rng.f32() * std::f32::consts::TAU;
            let mag = speed * (0.4 + rng.f32() * 0.6);
            let (s, c) = angle.sin_cos();
            self.particles.push(Particle {
                pos: origin,
                vel: Vec2::new(c * mag, s * mag),
                life,
                max_life: life,
                size: size * (0.6 + rng.f32() * 0.8),
                color,
                spin: (rng.f32() - 0.5) * 6.0,
                angle: rng.f32() * std::f32::consts::TAU,
            });
        }
    }

    pub fn emit_trail(&mut self, origin: Vec2, color: Color, rng: &mut impl RngLite) {
        if self.particles.len() >= self.capacity {
            return;
        }
        let angle = rng.f32() * std::f32::consts::TAU;
        let (s, c) = angle.sin_cos();
        let mag = 20.0 + rng.f32() * 40.0;
        self.particles.push(Particle {
            pos: origin + Vec2::new(c, s) * 4.0,
            vel: Vec2::new(c * mag * 0.15, s * mag * 0.15 + 30.0),
            life: 0.4 + rng.f32() * 0.35,
            max_life: 0.75,
            size: 8.0 + rng.f32() * 14.0,
            color,
            spin: (rng.f32() - 0.5) * 4.0,
            angle,
        });
    }

    pub fn update(&mut self, dt: f32) {
        for p in &mut self.particles {
            p.life -= dt;
            p.pos += p.vel * dt;
            p.vel *= 1.0 - (1.2 * dt).min(0.5);
            p.vel.y -= 40.0 * dt; // gentle gravity
            p.angle += p.spin * dt;
        }
        self.particles.retain(|p| p.life > 0.0);
    }

    /// Append particle sprites into `out` (caller draws with soft-circle texture).
    pub fn collect_sprites(&self, out: &mut Vec<Sprite>) {
        for p in &self.particles {
            let t = (p.life / p.max_life).clamp(0.0, 1.0);
            let mut c = p.color;
            c.a *= t;
            let size = p.size * (0.5 + 0.5 * t);
            out.push(
                Sprite::new(p.pos, Vec2::splat(size))
                    .with_color(c)
                    .with_rotation(p.angle)
                    .with_z(0.5),
            );
        }
    }
}

/// Tiny deterministic RNG so we don't pull in rand on wasm without hassle.
pub trait RngLite {
    fn f32(&mut self) -> f32;
}

/// xorshift32
#[derive(Debug, Clone)]
pub struct XorShift32(u32);

impl XorShift32 {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
}

impl RngLite for XorShift32 {
    fn f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32) / (u32::MAX as f32)
    }
}
