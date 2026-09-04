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
    gravity_scale: f32,
}

impl Particle {
    /// Converts an external spawn into system-internal state; spawned
    /// particles face their travel direction and fall at ambient gravity.
    fn from_spawn(spawned: SpawnedParticle) -> Self {
        Self {
            pos: spawned.position,
            vel: spawned.velocity,
            life: spawned.life,
            max_life: spawned.life,
            size: spawned.size,
            color: spawned.color,
            spin: 0.0,
            angle: spawned.velocity.y.atan2(spawned.velocity.x),
            gravity_scale: 1.0,
        }
    }
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

    #[allow(clippy::too_many_arguments)] // Keeps call sites readable for one-shot particle effects.
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
                gravity_scale: 1.0,
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
            gravity_scale: 1.0,
        });
    }

    /// Feeds one externally spawned particle (see [`RateEmitter::tick`]) into
    /// the system, honoring capacity.
    pub fn emit_single(&mut self, spawned: SpawnedParticle) {
        if self.particles.len() >= self.capacity {
            return;
        }
        self.particles.push(Particle::from_spawn(spawned));
    }

    pub fn update(&mut self, dt: f32) {
        for p in &mut self.particles {
            p.life -= dt;
            p.pos += p.vel * dt;
            p.vel *= 1.0 - (1.2 * dt).min(0.5);
            p.vel.y -= 40.0 * p.gravity_scale * dt; // gentle gravity
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

/// One particle handed off from a rate emitter into
/// [`ParticleSystem::emit_single`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnedParticle {
    pub position: Vec2,
    pub velocity: Vec2,
    pub life: f32,
    pub size: f32,
    pub color: Color,
}

/// Authored tuning for a [`RateEmitter`].
#[derive(Debug, Clone, Copy)]
pub struct EmitterConfig {
    /// Average spawns per second; fractional rates accumulate deterministically.
    pub rate_per_sec: f32,
    /// Launch speed in units per second (jittered per spawn).
    pub speed: f32,
    /// Particle lifetime in seconds.
    pub life: f32,
    /// Base sprite size in units.
    pub size: f32,
    /// Sprite tint.
    pub color: Color,
    /// Multiplier on ambient gravity for this emitter's spawns (1.0 =
    /// system default). [`SpawnedParticle`] carries no gravity of its own,
    /// so gameplay layers interpreting spawns read it from here.
    pub gravity_scale: f32,
}

/// Steady-rate emitter with deterministic, framerate-independent accumulation.
///
/// Each tick accrues `rate_per_sec * dt` spawns; whole spawns are emitted and
/// the fractional remainder carries forward, so any frame pacing converges on
/// exactly the authored rate. Spawn positions jitter within ±6 units of the
/// origin on both axes to break up uniform bands.
#[derive(Debug, Clone)]
pub struct RateEmitter {
    origin: Vec2,
    config: EmitterConfig,
    accumulator: f32,
}

impl RateEmitter {
    pub fn new(origin: Vec2, config: EmitterConfig) -> Self {
        Self {
            origin,
            config,
            accumulator: 0.0,
        }
    }

    pub fn set_origin(&mut self, origin: Vec2) {
        self.origin = origin;
    }

    /// The authored tuning this emitter accumulates against.
    pub fn config(&self) -> EmitterConfig {
        self.config
    }

    /// Accrues `dt` seconds of spawns into `out`, carrying the fractional
    /// remainder for later ticks. Deterministic for a fixed `rng` seed.
    pub fn tick(&mut self, dt: f32, rng: &mut impl RngLite, out: &mut Vec<SpawnedParticle>) {
        let dt = dt.max(0.0);
        if !self.config.rate_per_sec.is_finite() || self.config.rate_per_sec <= 0.0 {
            return;
        }
        self.accumulator += self.config.rate_per_sec * dt;
        if !self.accumulator.is_finite() {
            self.accumulator = 0.0;
            return;
        }
        let whole = self.accumulator.floor();
        self.accumulator -= whole;
        for _ in 0..(whole as usize) {
            let angle = rng.f32() * std::f32::consts::TAU;
            let mag = self.config.speed * (0.4 + rng.f32() * 0.6);
            let (s, c) = angle.sin_cos();
            out.push(SpawnedParticle {
                position: self.origin
                    + Vec2::new((rng.f32() - 0.5) * 12.0, (rng.f32() - 0.5) * 12.0),
                velocity: Vec2::new(c * mag, s * mag),
                life: self.config.life,
                size: self.config.size,
                color: self.config.color,
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(rate_per_sec: f32) -> EmitterConfig {
        EmitterConfig {
            rate_per_sec,
            speed: 80.0,
            life: 1.0,
            size: 6.0,
            color: Color::rgb(0.7, 0.75, 0.85),
            gravity_scale: 1.0,
        }
    }

    #[test]
    fn rate_emitter_spawns_exactly_the_authored_rate_per_second() {
        let mut emitter = RateEmitter::new(Vec2::new(40.0, 20.0), test_config(10.0));
        let mut rng = XorShift32::new(0xBEEF);
        let mut spawned = Vec::new();
        emitter.tick(1.0, &mut rng, &mut spawned);
        assert_eq!(spawned.len(), 10);

        // Same seed replays the same spawns bit for bit.
        let mut replay = RateEmitter::new(Vec2::new(40.0, 20.0), test_config(10.0));
        let mut rng = XorShift32::new(0xBEEF);
        let mut again = Vec::new();
        replay.tick(1.0, &mut rng, &mut again);
        assert_eq!(spawned, again);

        // Every spawn jitters within ±6 units of the origin.
        for particle in &spawned {
            assert!(particle.position.x >= 34.0 && particle.position.x <= 46.0);
            assert!(particle.position.y >= 14.0 && particle.position.y <= 26.0);
        }
    }

    #[test]
    fn fractional_spawns_carry_their_remainder_across_ticks() {
        let mut emitter = RateEmitter::new(Vec2::ZERO, test_config(3.0));
        let mut rng = XorShift32::new(1);
        let mut spawned = Vec::new();
        emitter.tick(0.25, &mut rng, &mut spawned);
        assert_eq!(spawned.len(), 0, "0.75 accrued: nothing whole yet");
        emitter.tick(0.25, &mut rng, &mut spawned);
        assert_eq!(spawned.len(), 1, "accumulator crossed 1.0");
        emitter.tick(0.25, &mut rng, &mut spawned);
        emitter.tick(0.25, &mut rng, &mut spawned);
        assert_eq!(spawned.len(), 3, "one full second converges on the rate");
        // The accumulator drained to zero, so an idle tick spawns nothing.
        emitter.tick(0.0, &mut rng, &mut spawned);
        assert_eq!(spawned.len(), 3);
    }

    #[test]
    fn set_origin_relocates_subsequent_spawns() {
        let mut emitter = RateEmitter::new(Vec2::ZERO, test_config(10.0));
        emitter.set_origin(Vec2::new(500.0, -300.0));
        let mut rng = XorShift32::new(9);
        let mut spawned = Vec::new();
        emitter.tick(1.0, &mut rng, &mut spawned);
        assert_eq!(spawned.len(), 10);
        assert!(
            spawned
                .iter()
                .all(|p| p.position.x > 480.0 && p.position.y > -320.0),
            "spawns track the new origin"
        );
    }

    #[test]
    fn emit_single_respects_the_system_capacity() {
        let mut system = ParticleSystem::new(3);
        for _ in 0..5 {
            system.emit_single(SpawnedParticle {
                position: Vec2::ZERO,
                velocity: Vec2::new(30.0, 10.0),
                life: 1.0,
                size: 4.0,
                color: Color::WHITE,
            });
        }
        assert_eq!(system.len(), 3);
    }

    #[test]
    fn rate_emitter_output_feeds_the_particle_system() {
        let mut emitter = RateEmitter::new(Vec2::ZERO, test_config(4.0));
        let mut rng = XorShift32::new(42);
        let mut spawned = Vec::new();
        emitter.tick(1.0, &mut rng, &mut spawned);
        assert_eq!(spawned.len(), 4);

        let mut system = ParticleSystem::new(8);
        for particle in spawned {
            system.emit_single(particle);
        }
        assert_eq!(system.len(), 4);
        system.update(0.5);
        assert_eq!(system.len(), 4, "all particles still have life left");
        let mut sprites = Vec::new();
        system.collect_sprites(&mut sprites);
        assert_eq!(sprites.len(), 4);
    }
}
