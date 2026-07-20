//! Resettable state owned by an active Last Light mission.

use glam::Vec2;

use crate::units::UnitKind;

pub struct Relay {
    pub position: Vec2,
    pub progress: f32,
    pub active: bool,
}
pub struct FieldBeacon {
    pub position: Vec2,
}

/// A small, presentation-neutral campaign beat owned by one named field role.
///
/// Specialist objectives deliberately do not replace the mission victory
/// condition. They are authored alongside it so the campaign can teach a
/// role-specific job (for example, keeping a Surveyor on a signal array or an
/// Engineer on a damaged reactor) and later surface the result in dialogue,
/// rewards, or a branch consequence without making the renderer or simulation
/// guess at mission intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialistObjectiveKind {
    SurveyorScan,
    WardenHold,
}

impl SpecialistObjectiveKind {
    pub const fn required_unit(self) -> UnitKind {
        match self {
            Self::SurveyorScan => UnitKind::Surveyor,
            Self::WardenHold => UnitKind::Warden,
        }
    }

    /// Short authored copy shared by the objective beacon, telemetry strip,
    /// and focus toast. Keeping this beside the enum prevents a new role from
    /// silently inheriting the Surveyor's language in the renderer.
    pub const fn objective_label(self) -> &'static str {
        match self {
            Self::SurveyorScan => "SURVEYOR SCAN",
            Self::WardenHold => "WARDEN HOLD",
        }
    }

    pub const fn progress_label(self) -> &'static str {
        match self {
            Self::SurveyorScan => "SCAN ARRAY",
            Self::WardenHold => "HOLD RELAY",
        }
    }

    pub const fn completion_label(self) -> &'static str {
        match self {
            Self::SurveyorScan => "SCAN COMPLETE // ESCORT",
            Self::WardenHold => "HOLD COMPLETE // PUSH",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpecialistObjective {
    pub kind: SpecialistObjectiveKind,
    pub target: Vec2,
    pub radius: f32,
    pub required_seconds: f32,
}

impl SpecialistObjective {
    pub const fn new(
        kind: SpecialistObjectiveKind,
        target: Vec2,
        radius: f32,
        required_seconds: f32,
    ) -> Self {
        Self {
            kind,
            target,
            radius,
            required_seconds,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        validate_objective_geometry(self.target, self.radius, self.required_seconds)
    }

    pub fn is_satisfied(self, unit_kind: UnitKind, unit_position: Vec2) -> bool {
        unit_kind == self.kind.required_unit()
            && unit_position.is_finite()
            && unit_position.distance(self.target) <= self.radius
    }
}

/// Engineer-only repair beat kept as a separate contract so its target and
/// progress copy cannot inherit Surveyor/Warden assumptions. The runtime
/// consumes it through the same resettable objective state, while the data
/// shape remains explicit and terrain-auditable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineerRepairObjective {
    pub target: Vec2,
    pub radius: f32,
    pub required_seconds: f32,
}

impl EngineerRepairObjective {
    pub const fn new(target: Vec2, radius: f32, required_seconds: f32) -> Self {
        Self {
            target,
            radius,
            required_seconds,
        }
    }

    pub const fn required_unit(self) -> UnitKind {
        UnitKind::Engineer
    }

    pub fn validate(self) -> Result<(), &'static str> {
        validate_objective_geometry(self.target, self.radius, self.required_seconds)
    }

    pub fn is_satisfied(self, unit_kind: UnitKind, unit_position: Vec2) -> bool {
        unit_kind == self.required_unit()
            && unit_position.is_finite()
            && unit_position.distance(self.target) <= self.radius
    }
}

fn validate_objective_geometry(
    target: Vec2,
    radius: f32,
    required_seconds: f32,
) -> Result<(), &'static str> {
    if !target.is_finite() {
        return Err("specialist objective target must be finite");
    }
    if !radius.is_finite() || radius <= 0.0 {
        return Err("specialist objective radius must be finite and positive");
    }
    if !required_seconds.is_finite() || required_seconds <= 0.0 {
        return Err("specialist objective duration must be finite and positive");
    }
    Ok(())
}

/// Deterministic progress for one authored specialist objective.
///
/// Progress only advances while the correct unit is inside the objective
/// radius. Invalid/negative fixed-step input is ignored, and progress is
/// monotonic once earned so replay traces do not depend on frame cadence or
/// presentation timing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpecialistObjectiveState {
    pub progress_seconds: f32,
    pub completed: bool,
}

impl SpecialistObjectiveState {
    pub const fn new() -> Self {
        Self {
            progress_seconds: 0.0,
            completed: false,
        }
    }

    pub fn advance(
        &mut self,
        objective: SpecialistObjective,
        unit_kind: UnitKind,
        unit_position: Vec2,
        dt: f32,
    ) -> bool {
        if self.completed {
            return true;
        }
        self.advance_contract(
            objective.required_seconds,
            objective.validate().is_ok() && objective.is_satisfied(unit_kind, unit_position),
            dt,
        )
    }

    pub fn advance_engineer_repair(
        &mut self,
        objective: EngineerRepairObjective,
        unit_kind: UnitKind,
        unit_position: Vec2,
        dt: f32,
    ) -> bool {
        self.advance_contract(
            objective.required_seconds,
            objective.validate().is_ok() && objective.is_satisfied(unit_kind, unit_position),
            dt,
        )
    }

    fn advance_contract(&mut self, required_seconds: f32, satisfied: bool, dt: f32) -> bool {
        if self.completed || !satisfied || !dt.is_finite() || dt <= 0.0 {
            return self.completed;
        }
        self.progress_seconds = (self.progress_seconds + dt).min(required_seconds);
        if self.progress_seconds >= required_seconds {
            self.completed = true;
        }
        self.completed
    }

    pub fn fraction(self, objective: SpecialistObjective) -> f32 {
        self.fraction_for(objective.required_seconds)
    }

    pub fn engineer_repair_fraction(self, objective: EngineerRepairObjective) -> f32 {
        self.fraction_for(objective.required_seconds)
    }

    fn fraction_for(self, required_seconds: f32) -> f32 {
        if !required_seconds.is_finite() || required_seconds <= 0.0 {
            return 0.0;
        }
        (self.progress_seconds / required_seconds).clamp(0.0, 1.0)
    }
}

impl Default for SpecialistObjectiveState {
    fn default() -> Self {
        Self::new()
    }
}

/// A role-specific hold that only advances while the unit occupies the
/// authored terrain advantage at the target. Unlike a normal specialist
/// objective, this contract makes elevation/cover part of the job: moving the
/// Warden onto an open patch beside the ridge is not enough to secure it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainControlObjective {
    pub target: Vec2,
    pub radius: f32,
    pub required_unit: UnitKind,
    pub minimum_elevation: i8,
    pub minimum_cover: f32,
    pub required_seconds: f32,
}

impl TerrainControlObjective {
    pub const fn high_ground_hold(
        target: Vec2,
        radius: f32,
        required_unit: UnitKind,
        required_seconds: f32,
    ) -> Self {
        Self {
            target,
            radius,
            required_unit,
            minimum_elevation: 1,
            minimum_cover: 0.0,
            required_seconds,
        }
    }

    #[allow(dead_code)]
    pub const fn covered_hold(
        target: Vec2,
        radius: f32,
        required_unit: UnitKind,
        minimum_cover: f32,
        required_seconds: f32,
    ) -> Self {
        Self {
            target,
            radius,
            required_unit,
            minimum_elevation: 0,
            minimum_cover,
            required_seconds,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        if !self.target.is_finite() {
            return Err("terrain control target must be finite");
        }
        if !self.radius.is_finite() || self.radius <= 0.0 {
            return Err("terrain control radius must be finite and positive");
        }
        if self.minimum_elevation < 0 {
            return Err("terrain control elevation cannot be negative");
        }
        if !self.minimum_cover.is_finite() || !(0.0..=0.3).contains(&self.minimum_cover) {
            return Err("terrain control cover must stay within the engine's 0..0.3 contract");
        }
        if !self.required_seconds.is_finite() || self.required_seconds <= 0.0 {
            return Err("terrain control duration must be finite and positive");
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn unit_present(self, presence: TerrainControlPresence) -> bool {
        presence.unit_kind == self.required_unit
            && presence.unit_position.is_finite()
            && presence.unit_position.distance(self.target) <= self.radius
    }

    pub fn terrain_satisfies(self, elevation: i8, cover: f32) -> bool {
        self.validate().is_ok()
            && elevation >= self.minimum_elevation
            && cover.is_finite()
            && cover >= self.minimum_cover
    }
}

/// Renderer/simulation-neutral snapshot consumed by a terrain-control step.
/// Keeping the live readout together avoids a long argument list and makes it
/// explicit that terrain is sampled at the same fixed-step moment as contest
/// presence.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct TerrainControlPresence {
    pub unit_kind: UnitKind,
    pub unit_position: Vec2,
    pub terrain_elevation: i8,
    pub terrain_cover: f32,
    pub enemy_present: bool,
}

/// Outcome of advancing a terrain-control beat for one fixed step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TerrainControlAdvance {
    Waiting,
    WrongTerrain,
    Contested,
    Progressed,
    Completed,
}

/// Resettable progress for a terrain-control objective. Enemy presence stalls
/// rather than erases progress, so the player is rewarded for reclaiming a
/// ridge after a raid without making the beat frame-rate dependent.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct TerrainControlState {
    pub progress_seconds: f32,
    pub contested_seconds: f32,
    pub contested: bool,
    pub completed: bool,
}

#[allow(dead_code)]
impl TerrainControlState {
    pub const fn new() -> Self {
        Self {
            progress_seconds: 0.0,
            contested_seconds: 0.0,
            contested: false,
            completed: false,
        }
    }

    pub fn advance(
        &mut self,
        objective: TerrainControlObjective,
        presence: TerrainControlPresence,
        dt: f32,
    ) -> TerrainControlAdvance {
        if self.completed {
            self.contested = false;
            return TerrainControlAdvance::Completed;
        }
        if !dt.is_finite() || dt <= 0.0 || objective.validate().is_err() {
            self.contested = false;
            return TerrainControlAdvance::Waiting;
        }
        if !objective.unit_present(presence) {
            self.contested = false;
            return TerrainControlAdvance::Waiting;
        }
        if !objective.terrain_satisfies(presence.terrain_elevation, presence.terrain_cover) {
            self.contested = false;
            return TerrainControlAdvance::WrongTerrain;
        }
        self.contested = presence.enemy_present;
        if self.contested {
            self.contested_seconds += dt;
            return TerrainControlAdvance::Contested;
        }
        self.progress_seconds = (self.progress_seconds + dt).min(objective.required_seconds);
        if self.progress_seconds >= objective.required_seconds {
            self.completed = true;
            return TerrainControlAdvance::Completed;
        }
        TerrainControlAdvance::Progressed
    }

    pub fn fraction(self, objective: TerrainControlObjective) -> f32 {
        if !objective.required_seconds.is_finite() || objective.required_seconds <= 0.0 {
            return 0.0;
        }
        (self.progress_seconds / objective.required_seconds).clamp(0.0, 1.0)
    }
}

impl Default for TerrainControlState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SalvageNode {
    pub position: Vec2,
    pub remaining: u32,
    pub harvest_buffer: f32,
    pub kind: ResourceKind,
    /// Saturation keeps a rich node from becoming a single-unit chore. The
    /// cap is authored per node so the campaign can make a contested pocket
    /// feel different from a wide-open field without changing extraction
    /// rules.
    pub max_workers: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Salvage,
    Flux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarvestPhase {
    ToNode,
    Extracting,
    ToDepot,
}

#[derive(Debug, Clone, Copy)]
pub struct HarvestJob {
    pub node: usize,
    pub cargo: u32,
    pub phase: HarvestPhase,
}

/// An authored worker objective attached to one finite resource node.
///
/// This is deliberately presentation-neutral: the tactical renderer can
/// expose it as a mini-menu, a beacon, or a radio prompt, while the
/// simulation only needs the role and distance contracts below. A support
/// role is optional so early missions can teach extraction before asking the
/// player to hold a node under pressure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResourceObjective {
    pub node_index: usize,
    pub worker_kind: UnitKind,
    pub support_kind: Option<UnitKind>,
    pub worker_radius: f32,
    pub support_radius: f32,
    pub contest_radius: f32,
    pub required_seconds: f32,
}

impl ResourceObjective {
    pub const fn secure_node(
        node_index: usize,
        worker_kind: UnitKind,
        support_kind: Option<UnitKind>,
        worker_radius: f32,
        support_radius: f32,
        contest_radius: f32,
        required_seconds: f32,
    ) -> Self {
        Self {
            node_index,
            worker_kind,
            support_kind,
            worker_radius,
            support_radius,
            contest_radius,
            required_seconds,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        if !self.worker_radius.is_finite() || self.worker_radius <= 0.0 {
            return Err("resource objective worker radius must be finite and positive");
        }
        if self.support_kind.is_some()
            && (!self.support_radius.is_finite() || self.support_radius <= 0.0)
        {
            return Err("resource objective support radius must be finite and positive");
        }
        if !self.contest_radius.is_finite() || self.contest_radius <= 0.0 {
            return Err("resource objective contest radius must be finite and positive");
        }
        if self.contest_radius < self.worker_radius {
            return Err("resource objective contest radius must cover the worker radius");
        }
        if !self.required_seconds.is_finite() || self.required_seconds <= 0.0 {
            return Err("resource objective duration must be finite and positive");
        }
        Ok(())
    }
}

/// Outcome of advancing a resource objective for one fixed step. The
/// explicit contested result lets simulation code publish one-shot warnings
/// without exposing internal timers to a renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceObjectiveAdvance {
    Waiting,
    Progressed,
    Contested,
    Completed,
}

/// Resettable state for one authored resource objective.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResourceObjectiveState {
    pub progress_seconds: f32,
    pub contested_seconds: f32,
    pub contested: bool,
    pub completed: bool,
}

impl ResourceObjectiveState {
    pub const fn new() -> Self {
        Self {
            progress_seconds: 0.0,
            contested_seconds: 0.0,
            contested: false,
            completed: false,
        }
    }

    pub fn advance(
        &mut self,
        objective: ResourceObjective,
        worker_present: bool,
        support_present: bool,
        enemy_present: bool,
        dt: f32,
    ) -> ResourceObjectiveAdvance {
        if self.completed {
            self.contested = false;
            return ResourceObjectiveAdvance::Completed;
        }
        if !dt.is_finite() || dt <= 0.0 || objective.validate().is_err() {
            self.contested = false;
            return ResourceObjectiveAdvance::Waiting;
        }

        // A node is contested only when a worker is actually trying to hold
        // it. Nearby hostiles still matter to combat, but do not create a
        // misleading objective warning before the player commits a worker.
        self.contested = worker_present && enemy_present && !support_present;
        if self.contested {
            self.contested_seconds += dt;
            return ResourceObjectiveAdvance::Contested;
        }
        if !worker_present {
            return ResourceObjectiveAdvance::Waiting;
        }

        self.progress_seconds = (self.progress_seconds + dt).min(objective.required_seconds);
        if self.progress_seconds >= objective.required_seconds {
            self.completed = true;
            return ResourceObjectiveAdvance::Completed;
        }
        ResourceObjectiveAdvance::Progressed
    }

    #[allow(dead_code)]
    pub fn fraction(self, objective: ResourceObjective) -> f32 {
        if !objective.required_seconds.is_finite() || objective.required_seconds <= 0.0 {
            return 0.0;
        }
        (self.progress_seconds / objective.required_seconds).clamp(0.0, 1.0)
    }
}

impl Default for ResourceObjectiveState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructureKind {
    Relay(usize),
    Fabricator,
    Reactor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructureState {
    pub kind: StructureKind,
    pub health: f32,
    pub max_health: f32,
    pub build_progress: f32,
    pub powered: bool,
}

impl StructureState {
    pub fn operational(self) -> bool {
        self.health > 0.0 && self.build_progress >= 1.0 && self.powered
    }
}

impl StructureKind {
    pub const RELAY_RADIUS: f32 = 85.0;
    pub const FABRICATOR_RADIUS: f32 = 105.0;
    pub const REACTOR_RADIUS: f32 = 135.0;
}
