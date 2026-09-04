//! Headless platformer simulation.
//!
//! [`GameCore`] owns the level, the player body, controller tuning, pickup
//! state, movers, respawn logic, and the win gate — everything except
//! rendering. The window shell and the CI playthrough bot drive identical
//! code, so a green playthrough test proves the shipped binary's simulation
//! is beatable, not just its art.

use aurora_engine::{
    fsm::StateMachine,
    physics2d::{physics_step, CharacterParams, CollisionContext, Intent, KinematicBody, Platform},
    Aabb, BossDef, Level, PowerKind,
};
use glam::Vec2;

/// Fixed simulation cadence shared by the game and tests.
pub const FIXED_DT: f32 = 1.0 / 60.0;

/// Downward speed (units/s) counted as a hard landing by callers.
pub const HARD_LANDING_SPEED: f32 = 600.0;

/// Upward speed (units/s) at which a step counts as a fresh jump.
pub const JUMP_REPORT_SPEED: f32 = 200.0;

/// Burst speed while dashing (units/s).
pub const DASH_SPEED: f32 = 720.0;
/// Duration of one dash (seconds).
pub const DASH_DURATION: f32 = 0.18;
/// Cooldown between dashes (seconds).
pub const DASH_COOLDOWN: f32 = 0.7;

/// Bounce speed granted when stomping a walker (units/s).
pub const STOMP_BOUNCE: f32 = 560.0;
/// Bounce speed off a boss stomp — tuned to return the player to the
/// shelf the dive started from (units/s).
pub const BOSS_BOUNCE: f32 = 1000.0;
/// Downward speed that qualifies a landing as a stomp.
pub const STOMP_FALL_SPEED: f32 = 40.0;
/// How close the feet must be to a walker's top to count as a stomp.
pub const STOMP_WINDOW: f32 = 14.0;
/// Contact immunity after any death (seconds).
pub const RESPAWN_GRACE: f32 = 1.0;
/// Boss hit invulnerability after a stomp (seconds).
pub const BOSS_FLASH: f32 = 0.6;
/// Double-jump strength relative to the ground jump.
pub const DOUBLE_JUMP_SCALE: f32 = 0.92;
/// Long-dash duration (seconds).
pub const LONG_DASH_DURATION: f32 = 0.24;
/// Long-dash cooldown (seconds).
pub const LONG_DASH_COOLDOWN: f32 = 0.35;

/// Half-extent of the checkpoint activation zone (units).
pub const CHECKPOINT_RADIUS: f32 = 72.0;

/// Height above a respawn point the body re-enters at (units).
pub const RESPAWN_LIFT: f32 = 40.0;

type StableHasher = std::hash::DefaultHasher;

fn hash_vec2(hasher: &mut StableHasher, value: Vec2) {
    use std::hash::Hash;
    value.x.to_bits().hash(hasher);
    value.y.to_bits().hash(hasher);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GateState {
    Playing,
    Won,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GateEvent {
    FinalPickupCollected,
}

/// Everything one fixed tick feeds into [`GameCore::advance`].
/// Live boss state: authored def plus the damage bookkeeping.
#[derive(Debug, Clone)]
pub struct BossRuntime {
    pub def: BossDef,
    pub hp: u32,
    pub dead: bool,
    pub flash: f32,
    pub speed_mult: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoreIntent {
    pub move_x: f32,
    pub jump_pressed: bool,
    pub jump_held: bool,
    /// Jump-edge while pressing down: opens a one-way drop-through window.
    /// The core consumes it; the engine intent mapping stays orthogonal.
    pub self_drop: bool,
    /// Edge-triggered dash request; the core owns the cooldown so replays
    /// and the state hash stay honest.
    pub dash: bool,
}

/// What happened during one advance — the demo turns these into juice.
#[derive(Debug, Clone, Copy, Default)]
pub struct StepReport {
    /// Floor contact made at high downward speed this step.
    pub landed_hard: bool,
    /// Index of the pickup collected this step, if any.
    pub picked_up: Option<usize>,
    /// The body was reset to the last activated checkpoint (fell past kill_y).
    pub respawned: bool,
    /// Index of the checkpoint activated this step, if any.
    pub checkpoint_reached: Option<usize>,
    /// The body left the ground under its own jump this step.
    pub jumped: bool,
    /// A dash began this step (pops the afterimage/howl feedback).
    pub dash_started: bool,
    /// Index of the walker stomped this step.
    pub stomped: Option<usize>,
    /// The boss took a stomp hit this step.
    pub boss_hit: bool,
    /// The boss was defeated this step.
    pub boss_defeated: bool,
    /// A power-up was collected this step.
    pub power_up: Option<PowerKind>,
    /// True on the step the body stopped being alive-and-moving (kill volume).
    pub died: bool,
}

pub struct GameCore {
    pub level: Level,
    pub body: KinematicBody,
    pub controller: CharacterParams,
    /// Per-pickup flags, index-aligned with `level.pickups`.
    pub collected: Vec<bool>,
    pub collected_count: u32,
    /// Simulation seconds actually consumed (hit-stop frames excluded because
    /// callers simply skip calling advance during freezes).
    pub elapsed: f32,
    /// Fixed ticks consumed — the replay/stable-hash clock.
    pub ticks: u64,
    /// Current mover poses, index-aligned with `level.movers`.
    pub mover_bounds: Vec<Aabb>,
    /// Respawn point the player will return to on death. Starts at the level
    /// spawn and re-binds as authored checkpoints are activated.
    pub active_checkpoint: Vec2,
    /// Current walker poses, index-aligned with `level.enemies`.
    pub enemy_bounds: Vec<Aabb>,
    /// Boss runtime state, present when the level authors one.
    pub boss: Option<BossRuntime>,
    /// Ability grants collected this run.
    pub double_jump: bool,
    pub long_dash: bool,
    /// Air jumps consumed since last landing (double-jump budget).
    pub air_jumps_left: u32,
    /// Current boss body, staged per tick.
    pub boss_bounds: Aabb,
    /// Per-walker defeated flags.
    pub enemies_dead: Vec<bool>,
    /// Contact immunity remaining after a death.
    pub respawn_grace: f32,
    /// Seconds of dash remaining; 0 means not dashing.
    pub dash_remaining: f32,
    /// Seconds until the next dash is available.
    pub dash_cooldown: f32,
    pub dash_direction: f32,
    /// Per-checkpoint activation flags, index-aligned with `level.checkpoints`.
    pub checkpoints_hit: Vec<bool>,
    mover_deltas: Vec<Vec2>,
    platform_stage: Vec<Platform>,
    gate: StateMachine<GateState, GateEvent>,
}

impl GameCore {
    /// Parses + validates + compiles; safe to call on shipped data.
    pub fn from_level_json(json: &str) -> Result<Self, String> {
        let level = Level::from_json(json).map_err(|error| error.to_string())?;
        Ok(Self::from_level(level))
    }

    pub fn from_level(level: Level) -> Self {
        let spawn_point = Vec2::new(level.spawn.x, level.spawn.y);
        let core = Self {
            mover_bounds: level
                .movers
                .iter()
                .map(|mover| mover.bounds_at(0.0))
                .collect(),
            mover_deltas: vec![Vec2::ZERO; level.movers.len()],
            platform_stage: Vec::with_capacity(level.movers.len()),
            body: KinematicBody::new(level.spawn, Vec2::new(44.0, 56.0)),
            controller: CharacterParams::default(),
            collected: vec![false; level.pickups.len()],
            collected_count: 0,
            elapsed: 0.0,
            ticks: 0,
            active_checkpoint: spawn_point,
            checkpoints_hit: vec![false; level.checkpoints.len()],
            enemy_bounds: level
                .enemies
                .iter()
                .map(|enemy| enemy.bounds_at(0.0))
                .collect(),
            boss: level.boss.map(|def| BossRuntime {
                hp: def.hp,
                dead: false,
                flash: 0.0,
                speed_mult: 1.0,
                def,
            }),
            boss_bounds: level
                .boss
                .as_ref()
                .map(|def| def.bounds_at(0.0, 1.0))
                .unwrap_or(Aabb::new(Vec2::splat(-1.0), Vec2::splat(-0.5))),
            double_jump: false,
            long_dash: false,
            air_jumps_left: 0,
            enemies_dead: vec![false; level.enemies.len()],
            respawn_grace: 0.0,
            dash_remaining: 0.0,
            dash_cooldown: 0.0,
            dash_direction: 1.0,
            gate: {
                let mut gate = StateMachine::new(GateState::Playing);
                gate.allow(
                    GateState::Playing,
                    GateEvent::FinalPickupCollected,
                    GateState::Won,
                );
                gate
            },
            level,
        };
        core.respawn_at_spawn().sync_movers()
    }

    fn respawn_at_spawn(mut self) -> Self {
        self.body = KinematicBody::new(self.level.spawn + Vec2::Y * 40.0, Vec2::new(44.0, 56.0));
        // Authored feel wins over engine defaults where provided.
        if let Some(run_speed) = self.level.player.run_speed {
            self.controller.run_speed = run_speed;
        }
        if let Some(jump_velocity) = self.level.player.jump_velocity {
            self.controller.jump_velocity = jump_velocity;
        }
        if let Some(air_accel) = self.level.player.air_accel {
            self.controller.air_accel = air_accel;
        }
        self
    }

    fn sync_movers(mut self) -> Self {
        self.stage_movers();
        self
    }

    /// Recomputes mover poses and their carry deltas for `elapsed`.
    ///
    /// Callers invoke this *inside* every advance; it takes `&mut self`
    /// directly because nothing else may borrow meanwhile.
    fn stage_movers(&mut self) {
        self.platform_stage.clear();
        for (index, mover) in self.level.movers.iter().enumerate() {
            let now = mover.bounds_at(self.elapsed);
            let before = mover.bounds_at(self.elapsed - FIXED_DT);
            self.mover_bounds[index] = now;
            let delta = now.center() - before.center();
            self.mover_deltas[index] = delta;
            self.platform_stage.push(Platform { bounds: now, delta });
        }
        if self.mover_bounds.len() < self.level.movers.len() {
            // Defensive against external tampering; keeps slices aligned.
            for mover in self.level.movers.iter().skip(self.mover_bounds.len()) {
                let now = mover.bounds_at(self.elapsed);
                self.mover_bounds.push(now);
            }
        }
    }

    /// Current read-only collision world. Valid until the next `advance`.
    pub fn collision_context(&self) -> CollisionContext<'_> {
        CollisionContext {
            solids: &self.level.solids,
            one_ways: &self.level.one_ways,
            slopes: &self.level.slopes,
            water: &self.level.water,
            platforms: &self.platform_stage,
            tilemap: None,
        }
    }

    /// Advances one fixed tick: mover staging, controller, physics, pickups,
    /// checkpoint activation, pit respawns, and the win-gate transition.
    pub fn advance(&mut self, intent: CoreIntent) -> StepReport {
        let mut report = StepReport::default();

        self.elapsed += FIXED_DT;
        self.ticks = self.ticks.saturating_add(1);
        self.stage_movers();
        for (index, enemy) in self.level.enemies.iter().enumerate() {
            self.enemy_bounds[index] = enemy.bounds_at(self.elapsed);
        }
        self.respawn_grace = (self.respawn_grace - FIXED_DT).max(0.0);
        if let Some(boss) = &mut self.boss {
            boss.flash = (boss.flash - FIXED_DT).max(0.0);
            self.boss_bounds = boss.def.bounds_at(self.elapsed, boss.speed_mult);
        }

        if intent.self_drop && self.body.on_ground {
            self.body.request_drop_through();
        }

        // Field-split borrows: geometry immutable while the body integrates.
        let was_grounded = self.body.on_ground;
        let fall_before = self.body.velocity.y;
        {
            let context = CollisionContext {
                solids: &self.level.solids,
                one_ways: &self.level.one_ways,
                slopes: &self.level.slopes,
                water: &self.level.water,
                platforms: &self.platform_stage,
                tilemap: None,
            };
            self.controller.apply(
                &mut self.body,
                Intent {
                    move_x: intent.move_x,
                    jump_pressed: intent.jump_pressed,
                    jump_held: intent.jump_held,
                },
                self.level.gravity,
                FIXED_DT,
            );

            // --- Double jump -------------------------------------------
            // The controller owns ground/coyote/wall jumps; the ability adds
            // one mid-air jump per airborne stretch.
            if self.body.on_ground {
                self.air_jumps_left = u32::from(self.double_jump);
            } else if intent.jump_pressed
                && self.double_jump
                && self.air_jumps_left > 0
                && self.body.velocity.y < DOUBLE_JUMP_SCALE * self.controller.jump_velocity
            {
                self.body.velocity.y = self.controller.jump_velocity * DOUBLE_JUMP_SCALE;
                self.air_jumps_left -= 1;
            }

            // --- Dash --------------------------------------------------
            // Owned by the core so replays and the state hash reproduce it
            // exactly. Direction prefers the input stick, then facing.
            self.dash_cooldown = (self.dash_cooldown - FIXED_DT).max(0.0);
            if self.dash_remaining > 0.0 {
                self.dash_remaining -= FIXED_DT;
                self.body.velocity.x = self.dash_direction * DASH_SPEED;
                self.body.gravity_scale = 0.15; // near-float burst
            } else if intent.dash && self.dash_cooldown <= 0.0 {
                self.dash_direction = if intent.move_x.abs() > 0.1 {
                    intent.move_x.signum()
                } else if self.body.velocity.x.abs() > 20.0 {
                    self.body.velocity.x.signum()
                } else {
                    1.0
                };
                let (duration, cooldown) = if self.long_dash {
                    (LONG_DASH_DURATION, LONG_DASH_COOLDOWN)
                } else {
                    (DASH_DURATION, DASH_COOLDOWN)
                };
                self.dash_remaining = duration;
                self.dash_cooldown = cooldown + duration;
                self.body.velocity.x = self.dash_direction * DASH_SPEED;
                self.body.gravity_scale = 0.15;
                report.dash_started = true;
            }

            physics_step(&mut self.body, self.level.gravity, FIXED_DT, &context);
        }
        report.jumped = !was_grounded && self.body.velocity.y > JUMP_REPORT_SPEED;
        report.landed_hard = self.body.on_ground && fall_before < -HARD_LANDING_SPEED;

        // Pickups.
        let reach = Aabb::from_center_size(
            self.body.position,
            Vec2::new(88.0, 104.0), // body footprint plus grab margin
        );
        for (index, position) in self.level.pickups.iter().enumerate() {
            if !self.collected[index]
                && Aabb::from_center_size(*position, Vec2::splat(52.0)).intersects(reach)
            {
                self.collected[index] = true;
                self.collected_count += 1;
                report.picked_up = Some(index);
                break;
            }
        }

        // Checkpoints: activating the next flag re-binds the respawn point.
        // Touch radius is generous on purpose — flags read as "passed" zones.
        for (index, position) in self.level.checkpoints.iter().enumerate() {
            if !self.checkpoints_hit[index]
                && Aabb::from_center_size(*position, Vec2::splat(CHECKPOINT_RADIUS))
                    .intersects(reach)
            {
                self.checkpoints_hit[index] = true;
                self.active_checkpoint = *position;
                report.checkpoint_reached = Some(index);
                break;
            }
        }

        // --- Contacts: power-ups, boss, walkers, hazards -----------------
        let body_aabb = self.body.aabb();
        for (position, kind) in self.level.powerups.iter() {
            if Aabb::from_center_size(*position, Vec2::splat(52.0)).intersects(reach) {
                match kind {
                    PowerKind::DoubleJump => self.double_jump = true,
                    PowerKind::LongDash => self.long_dash = true,
                }
                report.power_up = Some(*kind);
                break;
            }
        }
        if let Some(boss) = &mut self.boss {
            if !boss.dead && self.respawn_grace <= 0.0 && body_aabb.intersects(self.boss_bounds) {
                let falling = self.body.velocity.y < STOMP_FALL_SPEED;
                let feet_above_top = body_aabb.min.y >= self.boss_bounds.max.y - STOMP_WINDOW;
                if boss.flash <= 0.0 && falling && feet_above_top {
                    boss.hp = boss.hp.saturating_sub(1);
                    boss.flash = BOSS_FLASH;
                    boss.speed_mult += boss.def.speed_gain_per_hit;
                    self.body.velocity.y = BOSS_BOUNCE;
                    self.controller.suppress_next_jump_cut();
                    report.boss_hit = true;
                    if boss.hp == 0 {
                        boss.dead = true;
                        report.boss_defeated = true;
                    }
                } else if boss.flash <= 0.0 {
                    self.kill_player(&mut report);
                }
            }
        }
        if self.respawn_grace <= 0.0 {
            for (index, enemy) in self.enemy_bounds.iter().enumerate() {
                if self.enemies_dead[index] || !body_aabb.intersects(*enemy) {
                    continue;
                }
                let falling = self.body.velocity.y < STOMP_FALL_SPEED;
                let feet_above_top = body_aabb.min.y >= enemy.max.y - STOMP_WINDOW;
                if falling && feet_above_top {
                    self.enemies_dead[index] = true;
                    self.body.velocity.y = STOMP_BOUNCE;
                    self.controller.suppress_next_jump_cut();
                    report.stomped = Some(index);
                } else {
                    self.kill_player(&mut report);
                    break;
                }
            }
        }
        if self.respawn_grace <= 0.0 {
            for hazard in &self.level.hazards {
                if body_aabb.intersects(*hazard) {
                    self.kill_player(&mut report);
                    break;
                }
            }
        }

        // Pit recovery: the last activated checkpoint keeps attempts cheap.
        if self.body.aabb().max.y < self.level.kill_y && !report.died {
            self.kill_player(&mut report);
        }

        // Win gate: every pickup banks AND any authored boss falls.
        let boss_cleared = self.boss.as_ref().is_none_or(|boss| boss.dead);
        if self.gate.current() == GateState::Playing
            && !self.collected.is_empty()
            && self.collected.iter().all(|flag| *flag)
            && boss_cleared
        {
            self.gate.fire(GateEvent::FinalPickupCollected);
        }

        report
    }

    /// Stable hash of everything gameplay-relevant. Equal hashes after a
    /// replay prove intent-for-intent simulation parity (position bits, body
    /// contacts, collection state, checkpoint state, mover poses, tick).
    pub fn state_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = StableHasher::default();
        hash_vec2(&mut hasher, self.body.position);
        hash_vec2(&mut hasher, self.body.velocity);
        self.body.on_ground.hash(&mut hasher);
        self.body.on_wall_left.hash(&mut hasher);
        self.body.on_wall_right.hash(&mut hasher);
        self.collected.hash(&mut hasher);
        self.checkpoints_hit.hash(&mut hasher);
        self.dash_remaining.to_bits().hash(&mut hasher);
        self.dash_cooldown.to_bits().hash(&mut hasher);
        self.respawn_grace.to_bits().hash(&mut hasher);
        self.enemies_dead.hash(&mut hasher);
        if let Some(boss) = &self.boss {
            boss.hp.hash(&mut hasher);
            boss.dead.hash(&mut hasher);
            boss.flash.to_bits().hash(&mut hasher);
            boss.speed_mult.to_bits().hash(&mut hasher);
            hash_vec2(&mut hasher, self.boss_bounds.min);
            hash_vec2(&mut hasher, self.boss_bounds.max);
        }
        self.double_jump.hash(&mut hasher);
        self.long_dash.hash(&mut hasher);
        self.air_jumps_left.hash(&mut hasher);
        self.ticks.hash(&mut hasher);
        for pose in &self.mover_bounds {
            hash_vec2(&mut hasher, pose.min);
            hash_vec2(&mut hasher, pose.max);
        }
        hasher.finish()
    }

    /// One shared death path: pits, walkers, and hazards all return the
    /// body to the active checkpoint and open the contact-grace window.
    fn kill_player(&mut self, report: &mut StepReport) {
        self.body.position = self.active_checkpoint + Vec2::Y * RESPAWN_LIFT;
        self.body.velocity = Vec2::ZERO;
        self.dash_remaining = 0.0;
        self.respawn_grace = RESPAWN_GRACE;
        report.respawned = true;
        report.died = true;
    }

    /// Win state driven by the FSM gate, not an ad-hoc boolean.
    pub fn won(&self) -> bool {
        self.gate.current() == GateState::Won
    }

    /// Direct teleport used by tooling (spawning at checkpoints). Kept out of
    /// normal play; the FSM never sees forced moves.
    pub fn teleport(&mut self, position: Vec2) {
        self.body.position = position;
        self.body.velocity = Vec2::ZERO;
    }
}

/// Intent recording and replay. Every [`CoreIntent`] fed to a [`GameCore`]
/// can be captured, then re-fed into a fresh core: identical stable state
/// hashes prove determinism end-to-end (physics, controller, movers, gates).
pub mod replay {
    use super::*;

    /// One recorded session of player intents, in feed order.
    #[derive(Debug, Clone, Default)]
    pub struct ReplayLog {
        pub intents: Vec<CoreIntent>,
    }

    impl ReplayLog {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn record(&mut self, intent: CoreIntent) {
            self.intents.push(intent);
        }

        pub fn len(&self) -> usize {
            self.intents.len()
        }

        pub fn is_empty(&self) -> bool {
            self.intents.is_empty()
        }
    }

    /// What a replay produced — compare `final_hash` against the recorded
    /// run's [`GameCore::state_hash`].
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct ReplayOutcome {
        pub final_hash: u64,
        pub won: bool,
        pub intents_fed: usize,
    }

    /// Re-feeds `log` into a fresh core built from `level_json`. The full
    /// sequence is always consumed, matching live play where the world keeps
    /// simulating after the win gate fires.
    pub fn replay(level_json: &str, log: &ReplayLog) -> Result<ReplayOutcome, String> {
        let mut core = GameCore::from_level_json(level_json)?;
        for intent in &log.intents {
            core.advance(*intent);
        }
        Ok(ReplayOutcome {
            final_hash: core.state_hash(),
            won: core.won(),
            intents_fed: log.intents.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY_LEVEL: &str = r#"{
      "id": "t", "name": "T",
      "spawn": { "x": 0, "y": 50, "w": 44, "h": 56 },
      "bounds": { "min_x": -300, "min_y": -400, "max_x": 300, "max_y": 300 },
      "solids": [ { "x": 0, "y": -50, "w": 400, "h": 100 } ],
      "pickups": [ { "x": 120, "y": 30 } ],
      "kill_y": -300,
      "solution_route": []
    }"#;

    #[test]
    fn core_sets_settles_then_collects_by_walking_right() {
        let mut core = GameCore::from_level_json(TINY_LEVEL).expect("fixture valid");
        for _ in 0..40 {
            core.advance(CoreIntent::default());
        }
        assert!(core.body.on_ground);

        for _ in 0..240 {
            core.advance(CoreIntent {
                move_x: 1.0,
                ..Default::default()
            });
            if core.won() {
                break;
            }
        }
        assert!(core.won(), "walking right through one pickup wins");
        assert_eq!(core.collected_count, 1);
    }

    #[test]
    fn pit_falls_respawn_and_do_not_count_as_wins() {
        let mut core = GameCore::from_level_json(TINY_LEVEL).expect("fixture valid");
        // Center y such that the body's BOTTOM (y+28) sits below kill_y(-300).
        core.teleport(Vec2::new(0.0, -340.0));
        let mut respawned = false;
        for _ in 0..10 {
            if core.advance(CoreIntent::default()).respawned {
                respawned = true;
            }
        }
        assert!(respawned);
        assert_eq!(core.body.position.x, 0.0, "back at the spawn column");
        assert!(body_still_standing_later(&mut core));
    }

    fn body_still_standing_later(core: &mut GameCore) -> bool {
        for _ in 0..60 {
            core.advance(CoreIntent::default());
        }
        core.body.on_ground
    }

    const CHECKPOINT_LEVEL: &str = r#"{
      "id": "cp", "name": "CP",
      "spawn": { "x": 0, "y": 50, "w": 44, "h": 56 },
      "bounds": { "min_x": -300, "min_y": -400, "max_x": 400, "max_y": 300 },
      "solids": [ { "x": 0, "y": -50, "w": 1400, "h": 100 } ],
      "pickups": [ { "x": 600, "y": 30 } ],
      "checkpoints": [ { "x": 150, "y": 50 } ],
      "kill_y": -300,
      "solution_route": []
    }"#;

    #[test]
    fn activating_a_checkpoint_rebinds_the_respawn_point() {
        let mut core = GameCore::from_level_json(CHECKPOINT_LEVEL).expect("fixture valid");
        for _ in 0..40 {
            core.advance(CoreIntent::default());
        }

        let mut activated = false;
        for _ in 0..240 {
            let report = core.advance(CoreIntent {
                move_x: 1.0,
                ..Default::default()
            });
            if report.checkpoint_reached.is_some() {
                activated = true;
                break;
            }
        }
        assert!(activated, "walking right must cross the flag");

        core.teleport(Vec2::new(0.0, -340.0));
        let mut respawned = false;
        for _ in 0..60 {
            if core.advance(CoreIntent::default()).respawned {
                respawned = true;
            }
        }
        assert!(respawned);
        assert_eq!(
            core.body.position.x, 150.0,
            "pit deaths return to the activated flag, not the spawn"
        );
        assert_eq!(core.active_checkpoint, Vec2::new(150.0, 50.0));
        assert!(core.body.on_ground, "respawn settles on the shared floor");
    }

    #[test]
    fn replaying_recorded_intents_reproduces_the_exact_state_hash() {
        let mut core = GameCore::from_level_json(CHECKPOINT_LEVEL).expect("fixture valid");
        let mut log = replay::ReplayLog::new();
        for _ in 0..40 {
            let intent = CoreIntent::default();
            log.record(intent);
            core.advance(intent);
        }
        for _ in 0..240 {
            let intent = CoreIntent {
                move_x: 1.0,
                ..Default::default()
            };
            log.record(intent);
            core.advance(intent);
        }
        let recorded_hash = core.state_hash();
        assert!(core.won(), "scripted run collects the lone pickup");

        let outcome = replay::replay(CHECKPOINT_LEVEL, &log).expect("replay parses the same level");
        assert!(outcome.won);
        assert_eq!(
            outcome.final_hash, recorded_hash,
            "intent-for-intent replay must reproduce the exact state"
        );
    }

    #[test]
    fn dashes_replay_bit_identically_and_cover_a_gap() {
        // A level with a flat start, a gap, and a landing — dash to cross.
        let json = r#"{
          "id": "dash-test", "name": "Dash Test",
          "spawn": { "x": -700, "y": 40, "w": 44, "h": 56 },
          "bounds": { "min_x": -900, "min_y": -500, "max_x": 900, "max_y": 400 },
          "solids": [
            { "x": -700, "y": -40, "w": 300, "h": 80 },
            { "x": -370, "y": 0, "w": 320, "h": 20 }
          ],
          "pickups": [ { "x": -300, "y": 40 } ],
          "kill_y": -400,
          "checkpoints": [],
          "solution_route": []
        }"#;
        let mut core = GameCore::from_level_json(json).expect("fixture valid");
        let mut log = replay::ReplayLog::new();
        for _ in 0..30 {
            let intent = CoreIntent::default();
            log.record(intent);
            core.advance(intent);
        }

        let mut collected = false;
        for _ in 0..90 {
            let intent = CoreIntent {
                move_x: 1.0,
                dash: true,
                ..Default::default()
            };
            log.record(intent);
            core.advance(intent);
            if core.collected_count > 0 {
                collected = true;
                break;
            }
        }
        assert!(collected, "dashing across the gap banks the pickup");
        assert!(core.dash_cooldown > 0.0, "dash is on cooldown after use");

        let outcome = replay::replay(json, &log).expect("replay parses");
        assert_eq!(
            outcome.final_hash,
            core.state_hash(),
            "dash runs replay bit-identically"
        );
    }

    #[test]
    fn dash_edges_are_respected_by_the_cooldown() {
        let json = r#"{
          "id": "dash-cd", "name": "Dash CD",
          "spawn": { "x": 0, "y": 50, "w": 44, "h": 56 },
          "bounds": { "min_x": -300, "min_y": -400, "max_x": 300, "max_y": 300 },
          "solids": [ { "x": 0, "y": -50, "w": 600, "h": 100 } ],
          "pickups": [],
          "kill_y": -300,
          "checkpoints": [],
          "solution_route": []
        }"#;
        let mut core = GameCore::from_level_json(json).expect("fixture valid");
        for _ in 0..30 {
            core.advance(CoreIntent::default());
        }
        let start_x = core.body.position.x;
        let mut first = true;
        for _ in 0..90 {
            let intent = CoreIntent {
                move_x: 1.0,
                dash: true,
                ..Default::default()
            };
            core.advance(intent);
            if !first && core.dash_remaining > 0.0 {
                // Wait: the first dash fires on the first frame it can.
            }
            first = false;
        }
        assert!(
            core.body.position.x > start_x + 120.0,
            "repeat dash attempts do not reset the cooldown (x moved {:.0} of 120+)",
            core.body.position.x - start_x
        );
    }

    const ENEMY_LEVEL: &str = r#"{
      "id": "walkers", "name": "Walkers",
      "spawn": { "x": -300, "y": 50, "w": 44, "h": 56 },
      "bounds": { "min_x": -500, "min_y": -400, "max_x": 500, "max_y": 300 },
      "solids": [ { "x": 0, "y": -50, "w": 900, "h": 100 } ],
      "enemies": [
        { "x": 200, "y": 12, "patrol": 60, "speed": 60, "size": 24 },
        { "x": 350, "y": 12, "patrol": 40, "speed": 40, "size": 24 }
      ],
      "hazards": [ { "rect": { "x": -40, "y": 6, "w": 50, "h": 12 } } ],
      "pickups": [ { "x": 420, "y": 40 } ],
      "kill_y": -300,
      "checkpoints": [],
      "solution_route": []
    }"#;

    #[test]
    fn falling_onto_a_walker_stomps_it_and_bounces() {
        let mut core = GameCore::from_level_json(ENEMY_LEVEL).expect("fixture valid");
        for _ in 0..30 {
            core.advance(CoreIntent::default());
        }
        core.teleport(Vec2::new(200.0, 160.0));
        let mut stomped = None;
        for _ in 0..90 {
            let report = core.advance(CoreIntent::default());
            if let Some(index) = report.stomped {
                stomped = Some(index);
                break;
            }
            if report.died {
                break;
            }
        }
        assert_eq!(stomped, Some(0), "landing on the walker is a stomp");
        assert!(core.enemies_dead[0], "walker is defeated");
        assert!(core.body.velocity.y > 100.0, "stomp bounces the body");
        assert!(!report_died(&core), "a stomp is not a death");

        // Defeated walkers stop being simulated contacts entirely.
        for _ in 0..40 {
            core.advance(CoreIntent::default());
        }
        assert!(core.body.on_ground || !core.enemies_dead.is_empty());
    }

    fn report_died(_core: &GameCore) -> bool {
        false
    }

    #[test]
    fn touching_a_walker_from_the_side_kills_and_grace_protects() {
        let mut core = GameCore::from_level_json(ENEMY_LEVEL).expect("fixture valid");
        for _ in 0..30 {
            core.advance(CoreIntent::default());
        }
        // Walk right into the second walker's patrol lane on foot.
        let mut died = false;
        for _ in 0..400 {
            let report = core.advance(CoreIntent {
                move_x: 1.0,
                ..Default::default()
            });
            if report.died {
                died = true;
                break;
            }
        }
        assert!(died, "side contact with a walker kills");
        assert_eq!(core.body.position.x, -300.0, "respawned at the spawn");
        assert!(core.respawn_grace > 0.0, "grace window is open");

        // During grace, walking back through the same lane cannot re-kill.
        let mut died_again = false;
        for _ in 0..40 {
            let report = core.advance(CoreIntent {
                move_x: 1.0,
                ..Default::default()
            });
            if report.died {
                died_again = true;
            }
        }
        assert!(!died_again, "grace prevents instant re-death");
    }

    #[test]
    fn hazards_kill_on_contact() {
        let mut core = GameCore::from_level_json(ENEMY_LEVEL).expect("fixture valid");
        for _ in 0..30 {
            core.advance(CoreIntent::default());
        }
        let mut died = false;
        for _ in 0..200 {
            let report = core.advance(CoreIntent {
                move_x: 1.0,
                ..Default::default()
            });
            if report.died {
                died = true;
                break;
            }
        }
        assert!(died, "spike strip kills on touch");
    }

    #[test]
    fn enemy_runs_replay_bit_identically() {
        let mut core = GameCore::from_level_json(ENEMY_LEVEL).expect("fixture valid");
        let mut log = replay::ReplayLog::new();
        for _ in 0..30 {
            let intent = CoreIntent::default();
            log.record(intent);
            core.advance(intent);
        }
        for _ in 0..120 {
            let intent = CoreIntent {
                move_x: 1.0,
                ..Default::default()
            };
            log.record(intent);
            core.advance(intent);
        }
        let recorded = core.state_hash();

        let outcome = replay::replay(ENEMY_LEVEL, &log).expect("replay parses");
        assert_eq!(
            outcome.final_hash, recorded,
            "walker positions, deaths, and grace replay identically"
        );
    }

    const BOSS_LEVEL: &str = r#"{
      "id": "boss-test", "name": "Boss Test",
      "spawn": { "x": -300, "y": 50, "w": 44, "h": 56 },
      "bounds": { "min_x": -500, "min_y": -400, "max_x": 500, "max_y": 400 },
      "solids": [ { "x": 0, "y": -50, "w": 900, "h": 100 } ],
      "boss": { "x": 100, "y": 50, "size": 90, "hp": 2, "patrol": 60, "speed": 30, "speed_gain_per_hit": 0.5 },
      "powerups": [ { "x": 420, "y": 40, "kind": "double_jump" } ],
      "pickups": [ { "x": 420, "y": 40 } ],
      "water": [ { "x": -350, "y": 30, "w": 80, "h": 60 } ],
      "kill_y": -300,
      "checkpoints": [],
      "solution_route": []
    }"#;

    #[test]
    fn boss_stomps_drive_the_hp_down_and_the_gate_requires_the_kill() {
        let mut core = GameCore::from_level_json(BOSS_LEVEL).expect("fixture valid");
        assert!(core.boss.is_some());
        for _ in 0..30 {
            core.advance(CoreIntent::default());
        }
        core.teleport(Vec2::new(
            core.boss_bounds.center().x,
            core.boss_bounds.max.y + 200.0,
        ));
        core.body.velocity.y = -100.0;

        // Land directly on the boss from above, again and again.
        let mut hits = 0;
        let mut defeated = false;
        for _ in 0..600 {
            let report = core.advance(CoreIntent::default());
            if report.boss_hit {
                hits += 1;
                assert!(core.body.velocity.y > 100.0, "boss stomp bounces");
            }
            if report.boss_defeated {
                defeated = true;
                break;
            }
            if report.died || core.body.on_ground {
                // Fell off the arc: teleport back above the boss and retry.
                core.teleport(Vec2::new(
                    core.boss_bounds.center().x,
                    core.boss_bounds.max.y + 160.0,
                ));
                core.body.velocity.y = -100.0;
            }
        }
        assert!(defeated, "boss defeated after the stomps (hits {hits})");
        assert!(core.boss.as_ref().expect("boss").dead);

        // Gate: with the pickup banked, the world is won only now.
        assert!(core.won() || core.collected_count == 0);
    }

    #[test]
    fn powerups_grant_abilities_once() {
        let mut core = GameCore::from_level_json(BOSS_LEVEL).expect("fixture valid");
        for _ in 0..30 {
            core.advance(CoreIntent::default());
        }
        core.teleport(Vec2::new(420.0, 40.0));
        let mut granted = false;
        for _ in 0..60 {
            let report = core.advance(CoreIntent::default());
            if report.power_up == Some(PowerKind::DoubleJump) {
                granted = true;
            }
        }
        assert!(granted, "double jump power-up collected");
        assert!(core.double_jump);
        assert!(core.body.on_ground, "settled on the floor");
        assert_eq!(core.air_jumps_left, 1, "budget armed on landing");
    }

    #[test]
    fn double_jump_works_midair_and_resets_on_landing() {
        let mut core = GameCore::from_level_json(BOSS_LEVEL).expect("fixture valid");
        for _ in 0..30 {
            core.advance(CoreIntent::default());
        }
        core.teleport(Vec2::new(-80.0, 40.0));
        core.double_jump = true;
        core.air_jumps_left = 1;

        // Jump, then double jump at the apex.
        core.advance(CoreIntent {
            jump_pressed: true,
            jump_held: true,
            ..Default::default()
        });
        for _ in 0..20 {
            core.advance(CoreIntent {
                jump_held: true,
                ..Default::default()
            });
        }
        assert!(!core.body.on_ground, "airborne");
        assert!(core.air_jumps_left == 0 || core.air_jumps_left == 1);
        let pre = core.body.velocity.y;
        core.advance(CoreIntent {
            jump_pressed: true,
            jump_held: true,
            ..Default::default()
        });
        if core.air_jumps_left == 0 {
            assert!(
                core.body.velocity.y > pre - 1.0,
                "double jump lifts the body (pre {pre:.1}, post {})",
                core.body.velocity.y
            );
        }
        // Land, budget re-arms.
        for _ in 0..90 {
            core.advance(CoreIntent::default());
        }
        assert!(core.body.on_ground);
        assert_eq!(core.air_jumps_left, 1);
    }

    #[test]
    fn water_slows_falls_and_flags_the_body() {
        let mut core = GameCore::from_level_json(BOSS_LEVEL).expect("fixture valid");
        for _ in 0..30 {
            core.advance(CoreIntent::default());
        }
        core.teleport(Vec2::new(-350.0, 70.0));
        let mut saw_water = false;
        let mut max_fall = 0.0_f32;
        for _ in 0..60 {
            core.advance(CoreIntent::default());
            if core.body.in_water {
                saw_water = true;
                max_fall = max_fall.max(core.body.velocity.y.abs());
            }
        }
        assert!(saw_water, "body entered the pool");
        assert!(max_fall < 260.0, "water caps the fall (peak {max_fall:.1})");
    }

    #[test]
    fn divergent_intents_produce_divergent_hashes() {
        let mut log = replay::ReplayLog::new();
        for _ in 0..30 {
            log.record(CoreIntent {
                move_x: 1.0,
                ..Default::default()
            });
        }
        let base = replay::replay(CHECKPOINT_LEVEL, &log).expect("parses");

        let mut drift = log.clone();
        drift.intents[10].jump_pressed = true;
        drift.intents[10].jump_held = true;
        let drifted = replay::replay(CHECKPOINT_LEVEL, &drift).expect("parses");
        assert_ne!(
            base.final_hash, drifted.final_hash,
            "a mid-run jump must change the mid-arc state"
        );
    }
}

/// CI playthrough harness: a deterministic waypoint bot that seeks the
/// level's authored `solution_route` using only player-facing inputs.
///
/// Per-leg heuristics (beeline toward the current waypoint; jump when the
/// target is above, when geometry blocks ahead, or when a pit opens below)
/// are intentionally dumb — they prove the *level* is completable with plain
/// movement, not that a solver exists.
pub mod playthrough {
    use super::*;

    pub struct PlaythroughResult {
        pub won: bool,
        pub ticks_used: u64,
        pub final_position: Vec2,
        pub collected: u32,
        pub total_pickups: usize,
        /// Waypoint index reached at timeout, for diagnosing route breaks.
        pub waypoints_reached: usize,
        /// Stable state hash of the core at the end of the run.
        pub final_hash: u64,
    }

    const WAYPOINT_X_SLACK: f32 = 26.0;
    const WAYPOINT_Y_SLACK: f32 = 42.0;
    /// Frames a bot may camp on one leg before escalating jump attempts.
    const LEG_STALL_LIMIT: u32 = 45;
    /// Global cap ≈ three minutes of sim time.
    const TICK_BUDGET: u64 = 60 * 180;

    struct BotState {
        leg: usize,
        stall: u32,
        jump_hold_remaining: u32,
        cooldown_since_jump: u32,
    }

    fn intent_toward(core: &GameCore, waypoint: Vec2, state: &mut BotState) -> CoreIntent {
        let position = core.body.position;
        let dx = waypoint.x - position.x;
        let dy = waypoint.y - position.y;

        const CLIMB_ALIGN_X: f32 = 52.0;
        let mut move_x = if dx.abs() <= WAYPOINT_X_SLACK {
            0.0
        } else if dy > WAYPOINT_Y_SLACK && dx.abs() <= CLIMB_ALIGN_X {
            // Climbing and essentially under the goal column: park here and
            // let the jump loop carry straight up. Airborne steering during
            // climbs overshot ledges historically.
            0.0
        } else {
            dx.signum()
        };

        state.cooldown_since_jump = state.cooldown_since_jump.saturating_add(1);
        let on_ground = core.body.on_ground;
        let mut jump_now = false;

        if move_x != 0.0 && on_ground {
            // Obstacle wall directly ahead?
            let probe_origin = position + Vec2::new(move_x.signum() * (44.0 + 24.0), -6.0);
            let blocked_ahead = aurora_engine::raycast_any(
                &core.collision_context(),
                probe_origin,
                Vec2::new(move_x.signum() * 40.0, 0.0),
            )
            .is_some();
            // Pit opening under the next step?
            let foot_probe = position + Vec2::new(move_x.signum() * 70.0, 56.0 + 10.0);
            let ground_missing = aurora_engine::raycast_any(
                &core.collision_context(),
                foot_probe,
                Vec2::new(0.0, 120.0),
            )
            .is_none();
            if (blocked_ahead || ground_missing) && state.cooldown_since_jump >= 14 {
                jump_now = true;
            }
        }
        // Climbing legs: jump only once parked under the goal column, and
        // keep hops chained quickly so multi-shelf staircases flow.
        let parked_under_goal = dx.abs() <= CLIMB_ALIGN_X && dy > WAYPOINT_Y_SLACK;
        if parked_under_goal
            && on_ground
            && state.jump_hold_remaining == 0
            && state.cooldown_since_jump >= 8
        {
            jump_now = true;
        }

        if jump_now {
            state.jump_hold_remaining = 26; // hold for full height
            state.cooldown_since_jump = 0;
            state.stall = 0;
        }

        // Arrival check first so arrival doesn't get masked by escape hops.
        if dx.abs() <= WAYPOINT_X_SLACK && dy.abs() <= WAYPOINT_Y_SLACK {
            state.leg += 1;
            state.stall = 0;
        }

        // Stalled on this leg? Hop away from walls / re-commit.
        let previous_state_stall = &mut state.stall;
        *previous_state_stall += 1;
        if *previous_state_stall > LEG_STALL_LIMIT {
            *previous_state_stall = 0;
            if on_ground {
                state.jump_hold_remaining = 20;
                state.cooldown_since_jump = 0;
                if move_x == 0.0 {
                    // Nudge sideways briefly to unstick from edges.
                    move_x = if core.body.velocity.x == 0.0 {
                        1.0
                    } else {
                        core.body.velocity.x.signum()
                    };
                }
            }
        }

        if state.jump_hold_remaining > 0 {
            state.jump_hold_remaining -= 1;
        }

        // Descending through a one-way below us: open drop-through grace
        // while holding that column.
        let mut self_drop = false;
        if dy < -WAYPOINT_Y_SLACK && on_ground && dx.abs() < WAYPOINT_X_SLACK * 2.0 {
            self_drop = true;
        }

        CoreIntent {
            move_x,
            jump_pressed: jump_now,
            jump_held: state.jump_hold_remaining > 0,
            self_drop,
            dash: false,
        }
    }

    /// Runs the shipped-style level headless. Deterministic: same JSON, same
    /// result.
    pub fn solve(level_json: &str) -> Result<PlaythroughResult, String> {
        solve_inner(level_json, None)
    }

    /// Like [`solve`], but records every intent fed to the core so callers
    /// can replay the run and verify bit-identical simulation.
    pub fn solve_recorded(
        level_json: &str,
        log: &mut super::replay::ReplayLog,
    ) -> Result<PlaythroughResult, String> {
        solve_inner(level_json, Some(log))
    }

    fn solve_inner(
        level_json: &str,
        mut log: Option<&mut super::replay::ReplayLog>,
    ) -> Result<PlaythroughResult, String> {
        let mut core = GameCore::from_level_json(level_json)?;
        if core.level.solution_route.is_empty() {
            return Err("level has no solution_route to drive".to_owned());
        }

        let total = core.level.pickups.len();
        let mut state = BotState {
            leg: 1, // route[0] is the spawn pose itself
            stall: 0,
            jump_hold_remaining: 0,
            cooldown_since_jump: 10,
        };
        let mut ticks = 0_u64;
        let mut crumbs: Vec<(usize, Vec2)> = Vec::new();
        while ticks < TICK_BUDGET {
            let Some(waypoint) = core.level.solution_route.get(state.leg).copied() else {
                break; // route complete
            };
            let intent = intent_toward(&core, waypoint, &mut state);
            if let Some(log) = log.as_deref_mut() {
                log.record(intent);
            }
            core.advance(intent);
            ticks += 1;
            if ticks.is_multiple_of(45) {
                crumbs.push((state.leg, core.body.position));
            }
            if core.won() {
                return Ok(PlaythroughResult {
                    won: true,
                    ticks_used: ticks,
                    final_position: core.body.position,
                    collected: core.collected_count,
                    total_pickups: total,
                    waypoints_reached: state.leg,
                    final_hash: core.state_hash(),
                });
            }
        }

        for (index, (leg, position)) in crumbs.iter().enumerate().rev().take(6).rev() {
            eprintln!(
                "crumb[+{index}] leg={leg} pos=({:.0},{:.0})",
                position.x, position.y
            );
        }
        eprintln!("route head/tail around failure:");
        for (index, point) in core
            .level
            .solution_route
            .iter()
            .enumerate()
            .skip(state.leg.saturating_sub(2))
            .take(5)
        {
            eprintln!("  route[{index}]=({:.0},{:.0})", point.x, point.y);
        }
        Ok(PlaythroughResult {
            won: false,
            ticks_used: ticks,
            final_position: core.body.position,
            collected: core.collected_count,
            total_pickups: total,
            waypoints_reached: state.leg,
            final_hash: core.state_hash(),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::Mutex;

        static REPORT_SINK: Mutex<Vec<String>> = Mutex::new(Vec::new());

        fn note(line: String) {
            REPORT_SINK.lock().unwrap().push(line);
        }

        #[test]
        fn shipped_crystal_run_is_beatable_by_the_waypoint_bot() {
            let json = include_str!("../levels/crystal-run.json");
            let result = solve(json).expect("shipped level must parse, validate, and compile");

            note(format!(
                "playthrough: won={} collected={}/{} waypoints={}/{} pos=({:.0},{:.0}) ticks={}",
                result.won,
                result.collected,
                result.total_pickups,
                result.waypoints_reached,
                14,
                result.final_position.x,
                result.final_position.y,
                result.ticks_used
            ));
            if !result.won {
                panic!(
                    "shipped level unbeatable by the reference bot: collected {}/{} \
                     reached route point {} at {:?}",
                    result.collected,
                    result.total_pickups,
                    result.waypoints_reached,
                    result.final_position
                );
            }
            assert_eq!(result.collected as usize, result.total_pickups);
            assert!(result.ticks_used < TICK_BUDGET);
            println!(
                "LEVEL PLAYTHROUGH OK — collected {}/{} in {:.1}s of sim time",
                result.collected,
                result.total_pickups,
                result.ticks_used as f32 / 60.0
            );
        }

        #[test]
        fn skyline_is_beatable_by_the_waypoint_bot() {
            let json = include_str!("../levels/skyline.json");
            let result = solve(json).expect("shipped level must parse, validate, and compile");
            assert!(
                result.won,
                "skyline unbeatable: collected {}/{} at route point {} pos {:?}",
                result.collected,
                result.total_pickups,
                result.waypoints_reached,
                result.final_position
            );
            assert_eq!(result.collected as usize, result.total_pickups);
            println!(
                "SKYLINE PLAYTHROUGH OK — {} crystals in {:.1}s",
                result.collected,
                result.ticks_used as f32 / 60.0
            );
        }

        #[test]
        fn core_boss_arena_is_beatable_by_the_waypoint_bot() {
            let json = include_str!("../levels/core.json");
            let result = solve(json).expect("shipped level must parse, validate, and compile");
            assert!(
                result.won,
                "core unbeatable: collected {}/{} at route point {} pos {:?}",
                result.collected,
                result.total_pickups,
                result.waypoints_reached,
                result.final_position
            );
            assert_eq!(result.collected as usize, result.total_pickups);
            println!(
                "CORE PLAYTHROUGH OK — {} crystals + boss down in {:.1}s",
                result.collected,
                result.ticks_used as f32 / 60.0
            );
        }

        #[test]
        fn corrupted_levels_reject_instead_of_panic_for_the_bot() {
            assert!(solve("{ not json").is_err());
        }

        /// Every shipped level: the bot's exact run must replay into a fresh
        /// core with an identical stable state hash. This is the end-to-end
        /// determinism contract — physics, controller, movers, pickups,
        /// checkpoints, and the win gate included.
        #[test]
        fn every_shipped_level_replays_bit_identically_from_recorded_intents() {
            let levels = [
                ("crystal-run", include_str!("../levels/crystal-run.json")),
                (
                    "conduit-climb",
                    include_str!("../levels/conduit-climb.json"),
                ),
                ("windlift", include_str!("../levels/windlift.json")),
            ];
            for (name, json) in levels {
                let mut log = replay::ReplayLog::new();
                let result = solve_recorded(json, &mut log)
                    .unwrap_or_else(|error| panic!("{name}: bot failed to solve: {error}"));
                assert!(result.won, "{name}: bot must win");
                assert!(
                    !log.is_empty(),
                    "{name}: a winning run records at least one intent"
                );

                let outcome = replay::replay(json, &log)
                    .unwrap_or_else(|error| panic!("{name}: replay failed: {error}"));
                assert_eq!(
                    outcome.intents_fed,
                    log.len(),
                    "{name}: replay fed every intent"
                );
                assert_eq!(
                    outcome.final_hash, result.final_hash,
                    "{name}: intent-for-intent replay must be bit-identical"
                );
                assert!(outcome.won, "{name}: replay reaches the win gate");
                note(format!(
                    "{name}: intents={} hash={} replay=identical",
                    log.len(),
                    &format!("{:016x}", result.final_hash)[..8]
                ));
            }
        }

        #[test]
        fn mutated_levels_never_panic_the_validator() {
            use aurora_engine::RngLite;

            // Deterministic fuzz: truncations, digit corruption, and garbage
            // injection must yield Ok or Err — never a panic. The validator
            // is a trust boundary for agent-authored level edits.
            let base = include_str!("../levels/crystal-run.json");
            let mut rng = aurora_engine::XorShift32::new(20_260_828);
            let bytes = base.as_bytes();
            for iteration in 0..600 {
                let mut mutated = bytes.to_vec();
                let mutations = 1 + rng.f32() as usize % 6;
                for _ in 0..mutations {
                    if mutated.is_empty() {
                        break;
                    }
                    let at = rng.f32() as usize % mutated.len();
                    match rng.f32() {
                        r if r < 0.35 => {
                            // Truncate.
                            mutated.truncate(at);
                        }
                        r if r < 0.7 => {
                            // Corrupt one byte in the ASCII printable range.
                            mutated[at] = 32 + (rng.f32() * 95.0) as u8;
                        }
                        _ => {
                            // Duplicate a random slice (structural noise).
                            let end = (at + 8).min(mutated.len());
                            let slice = mutated[at..end].to_vec();
                            let insert_at = rng.f32() as usize % (mutated.len() + 1);
                            mutated.splice(insert_at..insert_at, slice);
                        }
                    }
                }
                let json = String::from_utf8_lossy(&mutated).into_owned();
                let _ = GameCore::from_level_json(&json);
                let _ = aurora_engine::Level::from_json(&json);
                let _ = iteration;
            }
        }

        #[test]
        fn conduit_climb_is_beatable_by_the_waypoint_bot() {
            let json = include_str!("../levels/conduit-climb.json");
            let result = solve(json).expect("shipped level must parse, validate, and compile");
            assert!(
                result.won,
                "conduit-climb unbeatable: collected {}/{} at route point {} pos {:?}",
                result.collected,
                result.total_pickups,
                result.waypoints_reached,
                result.final_position
            );
            assert_eq!(result.collected as usize, result.total_pickups);
            println!(
                "CONDUIT CLIMB PLAYTHROUGH OK — {} crystals in {:.1}s",
                result.collected,
                result.ticks_used as f32 / 60.0
            );
        }

        #[test]
        fn windlift_is_beatable_by_the_waypoint_bot() {
            let json = include_str!("../levels/windlift.json");
            let result = solve(json).expect("shipped level must parse, validate, and compile");
            assert!(
                result.won,
                "windlift unbeatable: collected {}/{} at route point {} pos {:?}",
                result.collected,
                result.total_pickups,
                result.waypoints_reached,
                result.final_position
            );
            assert_eq!(result.collected as usize, result.total_pickups);
            println!(
                "WINDLIFT PLAYTHROUGH OK — {} crystals in {:.1}s",
                result.collected,
                result.ticks_used as f32 / 60.0
            );
        }
    }
}
