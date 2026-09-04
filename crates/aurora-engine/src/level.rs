//! Data-driven level definitions for platformer and action games.
//!
//! A [`LevelDef`] is plain JSON (serde), authored alongside game assets and
//! embedded or fetched at will. [`LevelDef::validate`] fails closed with the
//! exact offending field so bad hand-edits cannot ship. [`Level::from_def`]
//! compiles a def into everything gameplay needs: solids, one-way ledges,
//! deterministic movers, pickups, respawn checkpoints, water volumes,
//! ambient emitters, power-ups, a boss encounter, and the authored theme —
//! with no renderer involved, so whole levels can be simulated headless in
//! tests.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::{Aabb, NavGrid, Slope};

/// One collider rectangle, author-friendly center/size form.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RectDef {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectDef {
    /// Center/size shorthand for authored maps.
    pub fn centered(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    fn validate(&self, what: &str, index: usize) -> Result<(), LevelError> {
        if !(self.x.is_finite() && self.y.is_finite() && self.w.is_finite() && self.h.is_finite()) {
            return Err(LevelError::NotFinite(format!("{what}[{index}]")));
        }
        if self.w <= 0.0 || self.h <= 0.0 {
            return Err(LevelError::NonPositiveSize(format!(
                "{what}[{index}] has size {}x{}",
                self.w, self.h
            )));
        }
        Ok(())
    }

    /// World-space AABB of this rect (center/size form).
    pub fn aabb(&self) -> Aabb {
        Aabb::from_center_size(Vec2::new(self.x, self.y), Vec2::new(self.w, self.h))
    }
}

/// Deterministic mover motion along one axis.
///
/// Position at time `t` is `phase_fn(t) = base + amplitude * sin(speed * t)`,
/// evaluated purely from elapsed level time — identical inputs replay
/// identically, satisfying the engine's determinism contracts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MoverDef {
    pub rect: RectDef,
    #[serde(default)]
    pub amplitude: f32,
    #[serde(default = "default_mover_speed")]
    pub speed: f32,
    #[serde(default)]
    pub vertical: bool,
    #[serde(default)]
    pub phase: f32,
}

/// A walkable ramp: linear surface from `surface_left` to `surface_right`
/// across the footprint's x-range. Ramps below the engine's steepness
/// threshold are walkable; steeper ones block like walls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlopeDef {
    /// Footprint (only the x-range and the two surface heights are physical).
    pub rect: RectDef,
    /// Surface height at the footprint's left edge.
    pub surface_left: f32,
    /// Surface height at the footprint's right edge.
    pub surface_right: f32,
}

impl SlopeDef {
    /// Converts into a physical [`Slope`].
    pub fn slope(&self) -> Slope {
        Slope {
            bounds: self.rect.aabb(),
            surface_left: self.surface_left,
            surface_right: self.surface_right,
        }
    }

    fn validate(&self, index: usize) -> Result<(), LevelError> {
        self.rect.validate("slopes", index)?;
        if !self.surface_left.is_finite() || !self.surface_right.is_finite() {
            return Err(LevelError::NotFinite(format!("slopes[{index}].surface")));
        }
        // Recorded geometry must never exceed the steepness the solver can
        // walk or the wall guard would fight the level author's intent.
        let ratio = (self.surface_right - self.surface_left).abs() / self.rect.w.max(f32::EPSILON);
        if ratio > 4.0 {
            return Err(LevelError::BadMover(format!(
                "slopes[{index}] rises {ratio:.2} units per unit of run; max walkable gradient is 4.0"
            )));
        }
        Ok(())
    }
}

/// A patrolling walker: ping-pongs horizontally around `x` by `patrol`
/// units at `speed`. Movement is a pure function of time, so replays and
/// state hashes reproduce it exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnemyDef {
    /// Patrol center x; the walker spans `[x - patrol, x + patrol]`.
    pub x: f32,
    /// Body center y (resting on its surface).
    pub y: f32,
    /// Half-range of the patrol in units.
    pub patrol: f32,
    /// Horizontal speed in units per second.
    pub speed: f32,
    /// Square body size in units.
    pub size: f32,
}

/// Ping-pong patrol offset shared by [`EnemyDef`] and [`BossDef`]: position
/// along `[-patrol, patrol]` after `rate * t` units of travel.
fn ping_pong_offset(patrol: f32, rate: f32, t: f32) -> f32 {
    if patrol <= f32::EPSILON {
        return 0.0;
    }
    let period = patrol * 2.0;
    let u = (t.max(0.0) * rate) % period;
    if u < patrol {
        -patrol + u
    } else {
        u - patrol
    }
}

impl EnemyDef {
    /// Body bounds at absolute time `t`.
    pub fn bounds_at(&self, t: f32) -> Aabb {
        let half = self.size * 0.5;
        let center_x = self.x + self.offset_at(t);
        Aabb::new(
            Vec2::new(center_x - half, self.y - half),
            Vec2::new(center_x + half, self.y + half),
        )
    }

    /// Ping-pong offset from the patrol center at time `t`.
    pub fn offset_at(&self, t: f32) -> f32 {
        ping_pong_offset(self.patrol, self.speed, t)
    }

    fn validate(&self, index: usize) -> Result<(), LevelError> {
        // Position may be negative; magnitudes may not.
        if !self.x.is_finite() || !self.y.is_finite() {
            return Err(LevelError::NotFinite(format!("enemies[{index}].position")));
        }
        for (name, value) in [
            ("patrol", self.patrol),
            ("speed", self.speed),
            ("size", self.size),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(LevelError::NotFinite(format!("enemies[{index}].{name}")));
            }
        }
        if self.size < 8.0 {
            return Err(LevelError::BadMover(format!(
                "enemies[{index}].size must be at least 8"
            )));
        }
        Ok(())
    }
}

/// A static damage zone (spike strip, thorn patch). Touching it kills.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HazardDef {
    pub rect: RectDef,
}

impl HazardDef {
    fn validate(&self, index: usize) -> Result<(), LevelError> {
        self.rect.validate("hazards", index)
    }
}

/// A soft ambient particle emitter region (dust motes, fireflies, ash).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AmbienceDef {
    pub rect: RectDef,
    /// Average spawns per second, validated to `0..=60`.
    pub rate_per_sec: f32,
}

impl AmbienceDef {
    fn validate(&self, index: usize) -> Result<(), LevelError> {
        self.rect.validate("ambience", index)?;
        if !self.rate_per_sec.is_finite() || !(0.0..=60.0).contains(&self.rate_per_sec) {
            return Err(LevelError::NotFinite(format!(
                "ambience[{index}].rate_per_sec must be finite in 0..=60"
            )));
        }
        Ok(())
    }
}

/// Permanent player upgrades scattered through a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerKind {
    DoubleJump,
    LongDash,
}

/// A power-up pickup placed in the world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PowerUpDef {
    pub x: f32,
    pub y: f32,
    pub kind: PowerKind,
}

impl PowerUpDef {
    fn validate(&self, index: usize) -> Result<(), LevelError> {
        if !self.x.is_finite() || !self.y.is_finite() {
            return Err(LevelError::NotFinite(format!("powerups[{index}]")));
        }
        Ok(())
    }
}

/// A boss encounter: a larger patrolling walker with hit points that gains
/// speed each time it is struck.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BossDef {
    /// Patrol center x; the boss spans `[x - patrol, x + patrol]`.
    pub x: f32,
    /// Body center y (resting on its surface).
    pub y: f32,
    /// Square body size in units.
    pub size: f32,
    /// Hits the player must land before the boss falls.
    pub hp: u32,
    /// Half-range of the patrol in units.
    pub patrol: f32,
    /// Base horizontal speed in units per second.
    pub speed: f32,
    /// Speed added per hit taken; games pass the running multiplier to
    /// [`Self::bounds_at`].
    pub speed_gain_per_hit: f32,
}

impl BossDef {
    /// Body bounds at absolute time `t` with the current hit-speed
    /// multiplier applied (games raise it by `speed_gain_per_hit` per strike).
    pub fn bounds_at(&self, t: f32, speed_mult: f32) -> Aabb {
        let half = self.size * 0.5;
        let center_x = self.x + ping_pong_offset(self.patrol, self.speed * speed_mult, t);
        Aabb::new(
            Vec2::new(center_x - half, self.y - half),
            Vec2::new(center_x + half, self.y + half),
        )
    }

    fn validate(&self) -> Result<(), LevelError> {
        // Position may be negative; magnitudes may not.
        if !self.x.is_finite() || !self.y.is_finite() {
            return Err(LevelError::NotFinite("boss.position".to_owned()));
        }
        for (name, value) in [
            ("patrol", self.patrol),
            ("speed", self.speed),
            ("size", self.size),
            ("speed_gain_per_hit", self.speed_gain_per_hit),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(LevelError::NotFinite(format!("boss.{name}")));
            }
        }
        if self.size < 16.0 {
            return Err(LevelError::BadMover(
                "boss.size must be at least 16".to_owned(),
            ));
        }
        if self.hp < 1 {
            return Err(LevelError::BadMover(
                "boss.hp must be at least 1".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Authored palette overriding the engine's default look.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThemeDef {
    /// Gradient top color as linear RGB in 0..=1.
    pub sky_top: [f32; 3],
    /// Gradient bottom color as linear RGB in 0..=1.
    pub sky_bottom: [f32; 3],
    /// Terrain sprite tint as linear RGB multipliers.
    pub terrain_tint: [f32; 3],
    /// UI/emphasis accent color as linear RGB in 0..=1.
    pub accent: [f32; 3],
    /// Ambient particle color as linear RGB in 0..=1.
    pub particle: [f32; 3],
}

impl Default for ThemeDef {
    fn default() -> Self {
        Self {
            sky_top: [0.05, 0.07, 0.16],
            sky_bottom: [0.10, 0.13, 0.28],
            terrain_tint: [1.0, 1.0, 1.0],
            accent: [0.18, 0.85, 0.72],
            particle: [0.7, 0.75, 0.85],
        }
    }
}

impl ThemeDef {
    fn validate(&self) -> Result<(), LevelError> {
        for (name, channel) in [
            ("sky_top", &self.sky_top),
            ("sky_bottom", &self.sky_bottom),
            ("terrain_tint", &self.terrain_tint),
            ("accent", &self.accent),
            ("particle", &self.particle),
        ] {
            if channel.iter().any(|value| !value.is_finite()) {
                return Err(LevelError::NotFinite(format!("theme.{name}")));
            }
        }
        Ok(())
    }
}

fn default_mover_speed() -> f32 {
    1.3
}

impl MoverDef {
    /// World-space bounds at absolute time `t`.
    pub fn bounds_at(&self, t: f32) -> Aabb {
        let offset = self.amplitude * (self.speed * t + self.phase).sin();
        let mut center = Vec2::new(self.rect.x, self.rect.y);
        if self.vertical {
            center.y += offset;
        } else {
            center.x += offset;
        }
        Aabb::from_center_size(center, Vec2::new(self.rect.w, self.rect.h))
    }

    fn validate(&self, index: usize) -> Result<(), LevelError> {
        self.rect.validate("movers", index)?;
        if !self.amplitude.is_finite() || self.amplitude < 0.0 {
            return Err(LevelError::BadMover(format!(
                "movers[{index}] amplitude must be finite >= 0"
            )));
        }
        if !self.speed.is_finite() || self.speed <= 0.0 {
            return Err(LevelError::BadMover(format!(
                "movers[{index}] speed must be finite > 0"
            )));
        }
        Ok(())
    }
}

/// A collectible placed in the world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PickupDef {
    pub x: f32,
    pub y: f32,
    #[serde(default = "default_pickup_radius")]
    pub radius: f32,
}

fn default_pickup_radius() -> f32 {
    30.0
}

impl PickupDef {
    fn position(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    fn validate(&self, index: usize) -> Result<(), LevelError> {
        if !self.x.is_finite() || !self.y.is_finite() || !self.radius.is_finite() {
            return Err(LevelError::NotFinite(format!("pickups[{index}]")));
        }
        if self.radius <= 0.0 {
            return Err(LevelError::NonPositiveSize(format!(
                "pickups[{index}] radius"
            )));
        }
        Ok(())
    }
}

/// Authored camera/world limits as `[min_x, min_y, max_x, max_y]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundsDef {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl BoundsDef {
    /// World-space AABB of this rect (center/size form).
    pub fn aabb(&self) -> Aabb {
        Aabb::new(
            Vec2::new(self.min_x, self.min_y),
            Vec2::new(self.max_x, self.max_y),
        )
    }

    fn validate(&self) -> Result<(), LevelError> {
        let ok = [self.min_x, self.min_y, self.max_x, self.max_y]
            .iter()
            .all(|value| value.is_finite());
        if !ok || self.max_x <= self.min_x || self.max_y <= self.min_y {
            return Err(LevelError::BadBounds(
                "bounds must be finite with max corners exceeding min".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Optional per-level character tuning so authored feel travels with the
/// map instead of living in engine defaults.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PlayerTuning {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_speed: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump_velocity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air_accel: Option<f32>,
}

impl PlayerTuning {
    fn validate(&self) -> Result<(), LevelError> {
        for (name, value) in [
            ("run_speed", self.run_speed),
            ("jump_velocity", self.jump_velocity),
            ("air_accel", self.air_accel),
        ] {
            if let Some(value) = value {
                if !value.is_finite() || value <= 0.0 {
                    return Err(LevelError::NotFinite(format!(
                        "player.{name} must be finite > 0"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// JSON-authored level definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelDef {
    pub id: String,
    pub name: String,
    /// World gravity magnitude in units/s².
    #[serde(default = "default_gravity")]
    pub gravity: f32,
    pub spawn: RectDef,
    pub bounds: BoundsDef,
    #[serde(default)]
    pub solids: Vec<RectDef>,
    #[serde(default)]
    pub one_ways: Vec<RectDef>,
    #[serde(default)]
    pub movers: Vec<MoverDef>,
    /// Walkable ramp surfaces.
    #[serde(default)]
    pub slopes: Vec<SlopeDef>,
    /// Patrolling walkers.
    #[serde(default)]
    pub enemies: Vec<EnemyDef>,
    /// Static damage zones.
    #[serde(default)]
    pub hazards: Vec<HazardDef>,
    #[serde(default)]
    pub pickups: Vec<PickupDef>,
    /// Swim volumes; buoyancy applies while a body's center is inside one.
    #[serde(default)]
    pub water: Vec<RectDef>,
    /// Ambient particle emitter regions.
    #[serde(default)]
    pub ambience: Vec<AmbienceDef>,
    /// Permanent player upgrades.
    #[serde(default)]
    pub powerups: Vec<PowerUpDef>,
    /// The level's boss encounter, if any.
    #[serde(default)]
    pub boss: Option<BossDef>,
    /// Authored palette; the engine default fills in when absent.
    #[serde(default)]
    pub theme: Option<ThemeDef>,
    /// Respawn points; falling below `kill_y` returns the player to the last
    /// checkpoint passed (the spawn itself is always checkpoint zero).
    pub kill_y: f32,
    #[serde(default)]
    pub checkpoints: Vec<PickupDef>,
    /// Authored solution route for tooling and CI playthrough bots. Purely
    /// advisory — gameplay never reads it; validation only checks finiteness.
    #[serde(default)]
    pub solution_route: Vec<[f32; 2]>,
    /// Optional override of player controller tuning.
    #[serde(default)]
    pub player: PlayerTuning,
}

fn default_gravity() -> f32 {
    1_900.0
}

/// Validation failures name the exact list index that is wrong.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelError {
    NotFinite(String),
    NonPositiveSize(String),
    BadMover(String),
    BadBounds(String),
    SpawnBlocked(u64),
    MissingSolidsUnderSpawn,
}

impl std::fmt::Display for LevelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFinite(what) => write!(formatter, "non-finite number in {what}"),
            Self::NonPositiveSize(what) => write!(formatter, "{what} must be positive"),
            Self::BadMover(what) => write!(formatter, "{what}"),
            Self::BadBounds(what) => write!(formatter, "{what}"),
            Self::SpawnBlocked(index) => {
                write!(formatter, "spawn overlaps solids[{index}]")
            }
            Self::MissingSolidsUnderSpawn => {
                write!(formatter, "no solid surface rests beneath the spawn")
            }
        }
    }
}

impl std::error::Error for LevelError {}

impl LevelDef {
    /// Parses JSON into a def without validating geometry.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Fails closed on anything unsafe to simulate.
    pub fn validate(&self) -> Result<(), LevelError> {
        if !self.gravity.is_finite() || self.gravity < 0.0 {
            return Err(LevelError::NotFinite("gravity".to_owned()));
        }
        self.player.validate()?;
        self.spawn.validate("spawn", usize::MAX)?;
        self.bounds.validate()?;
        for (index, solid) in self.solids.iter().enumerate() {
            solid.validate("solids", index)?;
        }
        for (index, ledge) in self.one_ways.iter().enumerate() {
            ledge.validate("one_ways", index)?;
        }
        for (index, mover) in self.movers.iter().enumerate() {
            mover.validate(index)?;
        }
        for (index, slope) in self.slopes.iter().enumerate() {
            slope.validate(index)?;
        }
        for (index, enemy) in self.enemies.iter().enumerate() {
            enemy.validate(index)?;
        }
        for (index, hazard) in self.hazards.iter().enumerate() {
            hazard.validate(index)?;
        }
        for (index, pickup) in self.pickups.iter().enumerate() {
            pickup.validate(index)?;
        }
        for (index, volume) in self.water.iter().enumerate() {
            volume.validate("water", index)?;
        }
        for (index, emitter) in self.ambience.iter().enumerate() {
            emitter.validate(index)?;
        }
        for (index, powerup) in self.powerups.iter().enumerate() {
            powerup.validate(index)?;
        }
        if let Some(boss) = &self.boss {
            boss.validate()?;
        }
        if let Some(theme) = &self.theme {
            theme.validate()?;
        }
        for (index, checkpoint) in self.checkpoints.iter().enumerate() {
            if !checkpoint.x.is_finite() || !checkpoint.y.is_finite() {
                return Err(LevelError::NotFinite(format!("checkpoints[{index}]")));
            }
        }
        for (index, waypoint) in self.solution_route.iter().enumerate() {
            let [x, y] = waypoint;
            if !x.is_finite() || !y.is_finite() {
                return Err(LevelError::NotFinite(format!("solution_route[{index}]")));
            }
        }

        // The player must not begin inside geometry.
        let spawn_bounds = self.spawn.aabb();
        for (index, solid) in self.solids.iter().enumerate() {
            if solid.aabb().intersects(spawn_bounds) {
                return Err(LevelError::SpawnBlocked(index as u64));
            }
        }

        // And must have somewhere reachable to stand (any floor overlapping a
        // column under the spawn box within half a body height).
        let under_spawn = Aabb::new(
            Vec2::new(spawn_bounds.min.x, spawn_bounds.min.y - self.spawn.h),
            Vec2::new(spawn_bounds.max.x, spawn_bounds.max.y),
        );
        let supported = self.solids.iter().any(|solid| {
            let floor = solid.aabb();
            floor.intersects(under_spawn) && floor.max.y <= spawn_bounds.max.y + 1.0
        });
        if !supported {
            return Err(LevelError::MissingSolidsUnderSpawn);
        }
        Ok(())
    }
}

/// Compiled level ready for simulation.
#[derive(Debug, Clone)]
pub struct Level {
    pub id: String,
    pub name: String,
    pub gravity: f32,
    pub solids: Vec<Aabb>,
    pub one_ways: Vec<Aabb>,
    pub movers: Vec<MoverDef>,
    pub slopes: Vec<Slope>,
    pub enemies: Vec<EnemyDef>,
    pub hazards: Vec<Aabb>,
    pub pickups: Vec<Vec2>,
    pub pickup_radius: f32,
    /// Swim volumes compiled to world-space AABBs.
    pub water: Vec<Aabb>,
    /// Ambient particle emitters, compiled as authored.
    pub ambience: Vec<AmbienceDef>,
    /// Power-up positions paired with the kind they grant.
    pub powerups: Vec<(Vec2, PowerKind)>,
    /// The boss encounter, if authored.
    pub boss: Option<BossDef>,
    /// Resolved palette (engine default when the def omitted one).
    pub theme: ThemeDef,
    pub spawn: Vec2,
    pub camera_bounds: Aabb,
    pub kill_y: f32,
    /// Respawn chain: implicit spawn first, then authored checkpoints by Y.
    pub respawns: Vec<Vec2>,
    /// Authored checkpoint positions in definition order.
    pub checkpoints: Vec<Vec2>,
    /// Authored route through the level (`LevelDef::solution_route`).
    pub solution_route: Vec<Vec2>,
    /// Resolved player overrides (engine defaults fill the gaps).
    pub player: PlayerTuning,
}

impl From<serde_json::Error> for LevelLoadError {
    fn from(error: serde_json::Error) -> Self {
        Self::Parse(error)
    }
}

impl std::error::Error for LevelLoadError {}

impl Level {
    /// Parses, validates, and compiles in one step.
    pub fn from_json(json: &str) -> Result<Self, LevelLoadError> {
        let def = LevelDef::from_json(json)?;
        Self::from_def(def).map_err(LevelLoadError::Invalid)
    }

    pub fn from_def(def: LevelDef) -> Result<Self, LevelError> {
        def.validate()?;
        let mut respawns = vec![Vec2::new(def.spawn.x, def.spawn.y)];
        let mut ordered_checkpoints: Vec<Vec2> = def
            .checkpoints
            .iter()
            .map(|point| point.position())
            .collect();
        ordered_checkpoints.sort_by(|a, b| b.y.total_cmp(&a.y)); // high ground first
        respawns.extend(ordered_checkpoints);

        Ok(Self {
            id: def.id.clone(),
            name: def.name.clone(),
            gravity: def.gravity,
            solids: def.solids.iter().map(|rect| rect.aabb()).collect(),
            one_ways: def.one_ways.iter().map(|rect| rect.aabb()).collect(),
            movers: def.movers.clone(),
            slopes: def.slopes.iter().map(|slope| slope.slope()).collect(),
            enemies: def.enemies.clone(),
            hazards: def
                .hazards
                .iter()
                .map(|hazard| hazard.rect.aabb())
                .collect(),
            pickups: def.pickups.iter().map(|pickup| pickup.position()).collect(),
            pickup_radius: def
                .pickups
                .iter()
                .map(|pickup| pickup.radius)
                .fold(f32::EPSILON, f32::max),
            water: def.water.iter().map(|rect| rect.aabb()).collect(),
            ambience: def.ambience,
            powerups: def
                .powerups
                .iter()
                .map(|powerup| (Vec2::new(powerup.x, powerup.y), powerup.kind))
                .collect(),
            boss: def.boss,
            theme: def.theme.unwrap_or_default(),
            spawn: Vec2::new(def.spawn.x, def.spawn.y),
            camera_bounds: def.bounds.aabb(),
            kill_y: def.kill_y,
            respawns,
            checkpoints: def
                .checkpoints
                .iter()
                .map(|point| point.position())
                .collect(),
            solution_route: def
                .solution_route
                .iter()
                .map(|point| Vec2::new(point[0], point[1]))
                .collect(),
            player: def.player,
        })
    }

    /// Mover platform state at absolute time `t`, per mover definition order.
    pub fn mover_platforms_at(&self, t: f32) -> Vec<(Aabb, Vec2)> {
        self.movers
            .iter()
            .scan(t - 1.0 / 240.0, |previous_t, mover| {
                let now = mover.bounds_at(t);
                let before = mover.bounds_at(*previous_t);
                *previous_t = t;
                Some((now, now.center() - before.center()))
            })
            .collect()
    }

    /// Optional nav grid covering this level (RTS/mixed-genre interop).
    ///
    /// Cell size trades resolution for width/height budget; callers choose.
    pub fn nav_grid(&self, cell: f32) -> NavGrid {
        let span = self.camera_bounds.size();
        let grid = NavGrid::new(
            (span.x / cell).ceil() as usize,
            (span.y / cell).ceil() as usize,
            self.camera_bounds.min,
            cell,
        );
        // Painting requires &mut; done in-place here via interior pattern.
        let mut grid = grid;
        crate::ai::mark_obstacles(&mut grid, &self.solids);
        grid
    }
}

/// Combined parse/validation failure surfaced to embedders.
#[derive(Debug)]
pub enum LevelLoadError {
    Parse(serde_json::Error),
    Invalid(LevelError),
}

impl std::fmt::Display for LevelLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "level JSON error: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid level: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD_LEVEL: &str = r#"{
      "id": "crystal-run",
      "name": "CRYSTAL RUN",
      "gravity": 1900.0,
      "spawn": { "x": -1200, "y": 60, "w": 44, "h": 56 },
      "bounds": { "min_x": -1460, "min_y": -700, "max_x": 1500, "max_y": 620 },
      "solids": [
        { "x": -500, "y": -80, "w": 1800, "h": 160 },
        { "x": 900, "y": 200, "w": 80, "h": 400 }
      ],
      "one_ways": [
        { "x": 230, "y": 100, "w": 220, "h": 20 }
      ],
      "movers": [
        { "rect": { "x": -540, "y": -80, "w": 180, "h": 26 },
          "amplitude": 240, "speed": 1.3, "vertical": false, "phase": 0.0 }
      ],
      "pickups": [
        { "x": 230, "y": 145 }, { "x": -130, "y": 330, "radius": 40 }
      ],
      "kill_y": -500,
      "checkpoints": [
        { "x": 720, "y": 40 }, { "x": -300, "y": 380 }
      ]
    }"#;

    #[test]
    fn good_level_parses_validates_and_compiles() {
        let level = Level::from_json(GOOD_LEVEL).expect("authored fixture stays valid");
        assert_eq!(level.id, "crystal-run");
        assert_eq!(level.solids.len(), 2);
        assert_eq!(level.one_ways.len(), 1);
        assert_eq!(level.movers.len(), 1);
        assert_eq!(level.pickups.len(), 2);
        // Respawn chain starts at the implicit spawn; authored checkpoints
        // sort highest-ground-first deterministically.
        assert_eq!(level.respawns[0], Vec2::new(-1200.0, 60.0));
        assert!(level.respawns[1].y > level.respawns[2].y);
    }

    #[test]
    fn mover_positions_are_pure_functions_of_time() {
        let level = Level::from_json(GOOD_LEVEL).unwrap();
        let a = level.movers[0].bounds_at(3.25);
        let b = level.movers[0].bounds_at(3.25);
        assert_eq!(a, b);
        // And the delta helper returns bounds(t) with the carry computed
        // against the same 1/240s-earlier pose the integrator consumes.
        let platforms = level.mover_platforms_at(1.0);
        assert_eq!(platforms.len(), 1);
        let (bounds, delta) = &platforms[0];
        let mover = &level.movers[0];
        assert_eq!(*bounds, mover.bounds_at(1.0));
        let expected_delta =
            mover.bounds_at(1.0).center() - mover.bounds_at(1.0 - 1.0 / 240.0).center();
        assert!(
            (*delta - expected_delta).length() < 1e-4,
            "carry delta tracks the analytic motion ({delta} vs {expected_delta})"
        );
    }

    #[test]
    fn non_positive_solids_are_rejected_with_index() {
        let def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        let mut broken = def.clone();
        broken.solids[1] = RectDef::centered(0.0, 0.0, 0.0, 10.0);
        assert_eq!(
            broken.validate(),
            Err(LevelError::NonPositiveSize(
                "solids[1] has size 0x10".to_owned()
            ))
        );
    }

    #[test]
    fn spawns_blocking_solids_fail_validation_naming_the_solid() {
        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.spawn = RectDef::centered(900.0, 200.0, 44.0, 56.0); // inside solid[1]
        assert_eq!(def.validate(), Err(LevelError::SpawnBlocked(1)));
    }

    #[test]
    fn floating_spawns_without_ground_support_are_caught() {
        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.spawn = RectDef::centered(460.0, 420.0, 44.0, 56.0);
        assert_eq!(def.validate(), Err(LevelError::MissingSolidsUnderSpawn));
    }

    #[test]
    fn bad_numbers_and_bad_bounds_never_load() {
        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.gravity = f32::NAN;
        assert!(matches!(def.validate(), Err(LevelError::NotFinite(_))));

        let mut flipped = LevelDef::from_json(GOOD_LEVEL).unwrap();
        flipped.bounds.max_y = flipped.bounds.min_y;
        assert!(matches!(flipped.validate(), Err(LevelError::BadBounds(_))));
    }

    #[test]
    fn compiled_level_feeds_a_nav_grid_for_mixed_genres() {
        let level = Level::from_json(GOOD_LEVEL).unwrap();
        let grid = level.nav_grid(40.0);
        let inside_first_floor = grid.is_blocked_at(Vec2::new(-500.0, -80.0));
        assert!(inside_first_floor, "solid tiles paint blocked cells");
        let open_sky = grid.is_blocked_at(Vec2::new(-500.0, 300.0));
        assert!(!open_sky);
    }

    #[test]
    fn power_kinds_parse_snake_case() {
        assert_eq!(
            serde_json::from_str::<PowerKind>("\"double_jump\"").unwrap(),
            PowerKind::DoubleJump
        );
        assert_eq!(
            serde_json::from_str::<PowerKind>("\"long_dash\"").unwrap(),
            PowerKind::LongDash
        );
    }

    #[test]
    fn water_powerups_and_boss_compile() {
        // Absent fields default to empty/None for defs that omit them.
        let plain = Level::from_json(GOOD_LEVEL).unwrap();
        assert!(plain.water.is_empty());
        assert!(plain.powerups.is_empty());
        assert!(plain.boss.is_none());

        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.water = vec![RectDef::centered(0.0, -400.0, 300.0, 200.0)];
        def.powerups = vec![PowerUpDef {
            x: 100.0,
            y: 40.0,
            kind: PowerKind::DoubleJump,
        }];
        def.boss = Some(BossDef {
            x: 1200.0,
            y: 120.0,
            size: 48.0,
            hp: 12,
            patrol: 80.0,
            speed: 60.0,
            speed_gain_per_hit: 15.0,
        });
        let level = Level::from_def(def).unwrap();
        assert_eq!(level.water.len(), 1);
        assert_eq!(
            level.powerups,
            vec![(Vec2::new(100.0, 40.0), PowerKind::DoubleJump)]
        );

        // Boss bounds follow the shared ping-pong patrol, scaled by the
        // hit-speed multiplier.
        let boss = level.boss.expect("boss compiled");
        assert!((boss.bounds_at(0.0, 1.0).center().x - (1200.0 - 80.0)).abs() < 1e-4);
        let base = boss.bounds_at(0.5, 1.0).center().x;
        let fast = boss.bounds_at(0.5, 2.0).center().x;
        assert!(base != fast, "speed multiplier accelerates the patrol");
        assert_eq!(boss.bounds_at(0.5, 2.0), boss.bounds_at(0.5, 2.0));
    }

    #[test]
    fn theme_defaults_apply_when_absent_and_authored_themes_override() {
        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.theme = None;
        let level = Level::from_def(def).unwrap();
        assert_eq!(level.theme, ThemeDef::default());

        let mut authored = LevelDef::from_json(GOOD_LEVEL).unwrap();
        authored.theme = Some(ThemeDef {
            sky_top: [0.0, 0.0, 0.0],
            ..ThemeDef::default()
        });
        let level = Level::from_def(authored).unwrap();
        assert_eq!(level.theme.sky_top, [0.0, 0.0, 0.0]);
        assert_eq!(level.theme.accent, ThemeDef::default().accent);
    }

    #[test]
    fn new_fields_fail_validation_naming_the_offender() {
        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.water = vec![
            RectDef::centered(0.0, -300.0, 100.0, 100.0),
            RectDef::centered(0.0, 0.0, f32::NAN, 10.0),
        ];
        assert_eq!(
            def.validate(),
            Err(LevelError::NotFinite("water[1]".to_owned()))
        );

        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.powerups = vec![PowerUpDef {
            x: f32::INFINITY,
            y: 0.0,
            kind: PowerKind::LongDash,
        }];
        assert_eq!(
            def.validate(),
            Err(LevelError::NotFinite("powerups[0]".to_owned()))
        );

        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.ambience = vec![AmbienceDef {
            rect: RectDef::centered(0.0, 0.0, 10.0, 10.0),
            rate_per_sec: 61.0,
        }];
        assert!(matches!(
            def.validate(),
            Err(LevelError::NotFinite(message)) if message.contains("ambience[0]")
        ));

        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.boss = Some(BossDef {
            x: 0.0,
            y: 0.0,
            size: 8.0,
            hp: 3,
            patrol: 0.0,
            speed: 0.0,
            speed_gain_per_hit: 0.0,
        });
        assert!(matches!(
            def.validate(),
            Err(LevelError::BadMover(message)) if message.contains("boss.size")
        ));

        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.boss = Some(BossDef {
            x: 0.0,
            y: 0.0,
            size: 48.0,
            hp: 0,
            patrol: 0.0,
            speed: 0.0,
            speed_gain_per_hit: 0.0,
        });
        assert!(matches!(
            def.validate(),
            Err(LevelError::BadMover(message)) if message.contains("boss.hp")
        ));

        let mut def = LevelDef::from_json(GOOD_LEVEL).unwrap();
        def.theme = Some(ThemeDef {
            accent: [f32::NAN, 0.0, 0.0],
            ..ThemeDef::default()
        });
        assert!(matches!(
            def.validate(),
            Err(LevelError::NotFinite(message)) if message.contains("theme.accent")
        ));
    }
}
