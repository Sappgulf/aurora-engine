//! Renderer-free tactical state for Last Light missions.

use std::collections::{HashMap, HashSet, VecDeque};

use aurora_engine::{
    mark_obstacles, Aabb, BlockId, BuildId, BuildQueue, BuildRecipe,
    CombatProfile as EngineCombatProfile, CooldownBook, DeterministicSimulation, FactionId,
    NavGrid, PowerGrid, PowerNode, PowerNodeId,
    ProductionCancelError as EngineProductionCancelError, ProductionQueue, QueueError,
    ResourceBank, ResourceSet, RtsCombatResolver, RtsWorld, SemanticCommand, StableStateHasher,
    StateHash, SupplyLedger, SupplyQueueError, TechGraph, TechId, TerrainZone, UnitId, UnitOrder,
};
use glam::Vec2;
use serde::Serialize;

use crate::mission_state::{
    Relay, ResourceObjective, ResourceObjectiveState, StructureKind, StructureState,
};
use crate::missions::{MissionDef, VictoryCondition};
use crate::units::{UnitKind, CHOIR, PLAYER};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionProfile {
    pub speed_scale: f32,
    pub steering: f32,
    pub separation_radius: f32,
    pub separation_strength: f32,
    pub label: &'static str,
}

pub const MAP_SIZE: Vec2 = Vec2::new(3600.0, 2200.0);
pub const NAV_CELL_SIZE: f32 = 40.0;
const DEFAULT_FIXED_TICK_HZ: u32 = 60;
const EVENT_LOG_CAPACITY: usize = 256;
const COMBAT_BUFFER_CAPACITY: usize = 32;
const PATH_ADVANCE_BUFFER_CAPACITY: usize = 32;
/// The opening relay is a teaching beat. The Choir should have time to show
/// the first objective and let a player establish a line before it can fund a
/// raid; otherwise a passive first order reads like an unavoidable ambush.
const FIRST_ENEMY_RAID_DELAY: f32 = 78.0;
const ENEMY_RAID_INTERVAL: f32 = 42.0;
const FIRST_RAID_RELAY_DAMAGE: f32 = 10.0;
const STANDARD_RAID_RELAY_DAMAGE: f32 = 18.0;
const LATE_RAID_HEALTH_STEP: f32 = 0.12;
const LATE_RAID_SPEED_STEP: f32 = 0.05;
const LATE_RAID_RELAY_DAMAGE_STEP: f32 = 2.0;
/// A warning is published before a funded raid spawns. Eight seconds is long
/// enough for the player to read the comms line and drag the frontline back to
/// the threatened relay, while still making the warning actionable.
pub const RAID_WARNING_WINDOW: f32 = 8.0;
const ATTACK_TELEGRAPH_MIN_INTERVAL: f32 = 0.36;
const ENEMY_RETREAT_HEALTH_FRACTION: f32 = 0.35;
/// Command Surge is an anchor button, not only a burst-DPS toggle. The
/// defensive half creates a short window for Wardens to hold a relay while
/// an Engineer restores it, with deterministic values shared by every build.
const COMMAND_SURGE_DAMAGE_SCALE: f32 = 1.35;
const COMMAND_SURGE_DAMAGE_TAKEN_SCALE: f32 = 0.72;
/// Cancelling a queued unit returns most of the primary cost while preserving
/// a small commitment cost. The percentage is shared by native/browser builds
/// and exposed so HUD copy can describe the same rule as the simulation.
pub const QUEUE_CANCEL_REFUND_PERCENT: u8 = 75;
pub const FABRICATOR_NODE: PowerNodeId = PowerNodeId(0);
pub const TECH_RELAY_NETWORK: TechId = TechId(1);
pub const TECH_LUMEN_CORE: TechId = TechId(2);
const SUPPLY_MODULE_BUILD_ID: BuildId = BuildId(1);

pub const SELECT_ALL_ACTION: &str = "last_light.select_all";
pub const SELECT_KIND_ACTION: &str = "last_light.select_kind";
pub const MOVE_SELECTED_ACTION: &str = "last_light.move_selected";
pub const ATTACK_MOVE_SELECTED_ACTION: &str = "last_light.attack_move_selected";
pub const PATROL_SELECTED_ACTION: &str = "last_light.patrol_selected";
pub const QUEUE_UNIT_ACTION: &str = "last_light.queue_unit";
pub const ATTACK_KIND_ACTION: &str = "last_light.attack_kind";

/// A small, deterministic ability vocabulary shared by the tactical HUD and
/// the headless simulation. Keeping the effects here prevents native and
/// browser builds from disagreeing about cooldowns or damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SpecialAbility {
    CommandSurge,
    EmergencyRepair,
    ScanPulse,
}

impl SpecialAbility {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CommandSurge => "COMMAND SURGE",
            Self::EmergencyRepair => "EMERGENCY REPAIR",
            Self::ScanPulse => "SCAN PULSE",
        }
    }

    pub const fn speaker(self) -> &'static str {
        match self {
            Self::CommandSurge => "MARA VEY",
            Self::EmergencyRepair => "IVO ROOK",
            Self::ScanPulse => "SENA QUILL",
        }
    }

    /// Full recharge duration for the signature action. Keeping the authored
    /// value beside the simulation's activation code gives presentation a
    /// truthful denominator for cooldown rails without reaching into private
    /// timer constants.
    pub const fn cooldown_seconds(self) -> f32 {
        match self {
            Self::CommandSurge => 18.0,
            Self::EmergencyRepair => 20.0,
            Self::ScanPulse => 16.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbilityError {
    NotAvailable,
    Cooldown,
    NoTarget,
}

#[derive(Debug, Clone, Copy)]
pub struct SimulationModifiers {
    pub player_health: f32,
    pub player_speed: f32,
    pub starting_salvage: u32,
    pub relay_income_per_second: u32,
    pub relay_restore_rate: f32,
    pub production_time_scale: f32,
    pub player_damage_scale: f32,
    pub player_damage_taken_scale: f32,
}

impl Default for SimulationModifiers {
    fn default() -> Self {
        Self {
            player_health: 1.0,
            player_speed: 1.0,
            starting_salvage: 150,
            relay_income_per_second: 3,
            relay_restore_rate: 1.0,
            production_time_scale: 1.0,
            player_damage_scale: 1.0,
            player_damage_taken_scale: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionOutcome {
    InProgress,
    Victory,
    Defeat,
}

/// Presentation-neutral phase for the next Choir raid. The renderer can turn
/// these into a compact countdown or minimap marker without reaching into
/// private economy timers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RaidPhase {
    /// The first contact is intentionally delayed so the opening relay order
    /// can be learned before pressure arrives.
    Teaching,
    /// The Choir is accumulating enough resources for its next commitment.
    Banking,
    /// A raid is funded and the warning window is active.
    Warning,
    /// The previous raid has been spent and the next timer is counting down.
    Cooldown,
}

/// Stable, renderer-free forecast of the next enemy raid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaidState {
    /// One-based number of the next raid (the first raid is `1`).
    pub number: u32,
    pub phase: RaidPhase,
    pub kind: UnitKind,
    /// Seconds until the next spawn. During the warning this is the readable
    /// countdown; it is clamped at zero after a large fixed-step jump.
    pub seconds_remaining: f32,
    pub anchor: Vec2,
    pub spawn_position: Vec2,
    pub funded: bool,
}

/// Current tactical state for one unit. This is intentionally a value object:
/// callers can draw health bars, target brackets, and role badges without
/// borrowing the live `RtsWorld` for longer than a single query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatContactState {
    Idle,
    Moving,
    Attacking,
    Retreating,
    Holding,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombatContact {
    pub unit_id: UnitId,
    pub kind: UnitKind,
    pub faction: FactionId,
    pub position: Vec2,
    pub health_fraction: f32,
    pub state: CombatContactState,
    pub target: Option<UnitId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SimulationEventKind {
    CommandAccepted {
        action: String,
    },
    RelayActivated {
        index: usize,
    },
    ResourcesCredited {
        amount: u32,
    },
    ResourcesDelivered {
        salvage: u32,
        flux: u32,
    },
    UnitQueued {
        kind: UnitKind,
    },
    UnitDeployed {
        unit_id: u32,
        kind: UnitKind,
    },
    UnitSpawned {
        unit_id: u32,
        kind: UnitKind,
    },
    AttackLanded {
        attacker: u32,
        target: u32,
    },
    /// Published once when a one-shot area-denial unit triggers, before the
    /// unit is destroyed, so presentation can anchor VFX and haptics to the
    /// authoritative detonation position.
    UnitDetonated {
        unit_id: u32,
        kind: UnitKind,
    },
    /// Emitted when an attacker gains a target, including attack-move target
    /// acquisition. A HUD can keep a bracket on the target until `TargetLost`.
    TargetAcquired {
        attacker: u32,
        target: u32,
    },
    TargetLost {
        attacker: u32,
    },
    /// Emitted on a coarse cadence while a unit is in attack range. It gives
    /// presentation a wind-up beat before the continuous DPS tick lands.
    AttackTelegraph {
        attacker: u32,
        target: u32,
        windup_seconds: f32,
    },
    DamageApplied {
        target: u32,
    },
    UnitRepaired {
        engineer: u32,
        target: u32,
    },
    StructureRepaired {
        structure: String,
    },
    UnitDestroyed {
        unit_id: u32,
        kind: UnitKind,
    },
    BossReinforced,
    EnemyRaidSpawned {
        unit_id: u32,
        kind: UnitKind,
    },
    /// Published once during the final warning window before the next raid
    /// spawns. `spawn_x/y` let the minimap draw an approach marker without
    /// recomputing mission-specific offsets.
    EnemyRaidTelegraph {
        number: u32,
        kind: UnitKind,
        anchor_x: f32,
        anchor_y: f32,
        spawn_x: f32,
        spawn_y: f32,
        seconds_remaining: f32,
    },
    UnitRetreating {
        unit_id: u32,
        kind: UnitKind,
    },
    UnitRecovered {
        unit_id: u32,
        kind: UnitKind,
    },
    AbilityActivated {
        unit_id: u32,
        ability: SpecialAbility,
    },
    StructureBuildQueued {
        structure: String,
    },
    StructureBuildCompleted {
        structure: String,
    },
    MissionVictory,
    MissionDefeat,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimulationEvent {
    pub tick: u64,
    #[serde(flatten)]
    pub kind: SimulationEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionCommandError {
    UnitCap,
    SupplyBlocked,
    FabricatorOffline,
    UnsupportedUnit,
    InsufficientFlux,
    Queue(QueueError),
}

/// Failure reasons for cancelling a unit from the Fabricator queue. A
/// cancellation is intentionally a separate command from queue admission so
/// callers can give precise feedback for an offline structure or bad slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionCancelCommandError {
    FabricatorOffline,
    InvalidIndex,
    UnsupportedUnit,
    SupplyLedgerRequired,
}

/// Deterministic receipt for a Last Light queue cancellation. The engine
/// reports the primary refund and released supply; Last Light adds the
/// secondary Flux refund because it is a campaign-specific admission cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitQueueCancelReceipt {
    pub kind: UnitKind,
    pub refunded_salvage: u32,
    pub refunded_flux: u32,
    pub released_supply: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureCommandError {
    FabricatorOffline,
    Busy,
    Maxed,
    InsufficientResources,
}

/// The first extracted Last Light simulation seam. It owns the live tactical
/// roster, unit identity, navigation grid, and path following used by both the
/// interactive game and headless semantic traces.
pub struct MissionSimulation {
    pub world: RtsWorld,
    pub kinds: HashMap<UnitId, UnitKind>,
    /// Mission-authored callsigns for named field specialists. Keeping this
    /// alongside `kinds` means identity survives renderer selection and is
    /// available to both HUD and deterministic traces.
    pub identities: HashMap<UnitId, &'static str>,
    pub nav: NavGrid,
    pub player_paths: HashMap<UnitId, VecDeque<Vec2>>,
    pub escort_unit: Option<UnitId>,
    pub relays: Vec<Relay>,
    pub resources: ResourceBank,
    pub flux: u32,
    pub supply: SupplyLedger,
    pub tech: TechGraph,
    pub structures: HashMap<StructureKind, StructureState>,
    pub terrain_zones: Vec<TerrainZone>,
    /// Optional authored worker/resource contract. The target is stored
    /// separately because the mission definition is data-only and the
    /// simulation must retain the resolved world position after construction.
    resource_objective: Option<ResourceObjective>,
    resource_objective_target: Option<Vec2>,
    resource_objective_state: ResourceObjectiveState,
    pub reactor_position: Option<Vec2>,
    pub enemy_resources: ResourceSet,
    pub enemy_raid_count: u32,
    pub salvage_delivered: u32,
    pub production: ProductionQueue,
    /// Generic infrastructure queue backing structure construction. The
    /// public supply-module progress below remains a presentation mirror.
    structure_builds: BuildQueue,
    pub power: PowerGrid,
    pub fabricator_position: Vec2,
    pub rally_point: Option<Vec2>,
    pub outcome: MissionOutcome,
    /// Remaining seconds for a unit's tactical ability. Hashing these values
    /// makes ability use replay-safe just like movement and production.
    pub ability_cooldowns: CooldownBook,
    /// Command Surge is a short-lived per-unit combat buff.
    pub command_surges: CooldownBook,
    /// Surveyor scan pulses are read by the presentation layer to reveal fog
    /// and draw a ring, but the center/timer still belong to deterministic
    /// mission state.
    pub scan_pulse: Option<(Vec2, f32)>,
    /// Timed infrastructure construction owned by the Fabricator. A module
    /// spends Salvage when queued and only changes capacity when the build
    /// completes, so supply decisions have a readable commitment window.
    pub supply_module_progress: Option<f32>,
    pub supply_module_level: u8,
    tick: u64,
    fixed_dt: f32,
    resource_tick: f32,
    modifiers: SimulationModifiers,
    victory_condition: VictoryCondition,
    boss_reinforced: bool,
    events: VecDeque<SimulationEvent>,
    pending_events: VecDeque<SimulationEvent>,
    path_advance_ids: Vec<UnitId>,
    /// IDs currently following a routed AttackMove path. The public path
    /// queue stays a simple list of waypoints, while this sidecar preserves
    /// the order kind when each waypoint is promoted.
    attack_move_paths: HashSet<UnitId>,
    combat_snapshot: Vec<(UnitId, Vec2, bool)>,
    combat_attacks: Vec<(UnitId, UnitId, f32)>,
    /// Renderer-independent cadence/range/damage resolver used for ordinary
    /// weapon pulses. Game-specific target acquisition, telegraphs, and mine
    /// detonation remain in this simulation layer.
    combat_resolver: RtsCombatResolver,
    combat_detonations: Vec<UnitId>,
    /// Last target selected by combat resolution. Keeping this separate from
    /// `UnitOrder` also covers attack-move acquisition and gives the HUD a
    /// stable bracket target between damage ticks.
    combat_targets: HashMap<UnitId, UnitId>,
    attack_telegraph_timers: HashMap<UnitId, f32>,
    combat_target_updates: Vec<(UnitId, Option<UnitId>)>,
    combat_telegraph_candidates: Vec<(UnitId, UnitId)>,
    retreating_units: HashSet<UnitId>,
    support_repairs: Vec<(UnitId, UnitId, f32)>,
    structure_repairs: Vec<(UnitId, StructureKind, f32)>,
    enemy_income_tick: f32,
    enemy_raid_timer: f32,
    enemy_telegraph_emitted: bool,
    destroyed_by_kind: HashMap<UnitKind, u32>,
}

impl MissionSimulation {
    pub fn from_mission(mission: &MissionDef, modifiers: SimulationModifiers) -> Self {
        let mut nav = NavGrid::new(
            (MAP_SIZE.x / NAV_CELL_SIZE).ceil() as usize,
            (MAP_SIZE.y / NAV_CELL_SIZE).ceil() as usize,
            -MAP_SIZE * 0.5,
            NAV_CELL_SIZE,
        );
        let mut nav_obstacles = mission.obstacles.clone();
        nav_obstacles.push(Aabb::from_center_size(
            mission.fabricator_position,
            Vec2::splat(StructureKind::FABRICATOR_RADIUS * 2.0),
        ));
        nav_obstacles.extend(mission.relays.iter().copied().map(|position| {
            Aabb::from_center_size(position, Vec2::splat(StructureKind::RELAY_RADIUS * 2.0))
        }));
        if let Some(reactor_position) = mission.reactor_position {
            nav_obstacles.push(Aabb::from_center_size(
                reactor_position,
                Vec2::splat(StructureKind::REACTOR_RADIUS * 2.0),
            ));
        }
        mark_obstacles(&mut nav, &nav_obstacles);
        let mut power = PowerGrid::default();
        power.add_node(PowerNode {
            id: FABRICATOR_NODE,
            supply: 1,
            demand: 1,
            online: true,
        });
        for index in 0..mission.relays.len() {
            let relay = PowerNodeId(index as u16 + 1);
            power.add_node(PowerNode {
                id: relay,
                supply: 1,
                demand: 0,
                online: false,
            });
            power.link(FABRICATOR_NODE, relay);
        }
        let mut tech = TechGraph::default();
        tech.define(TECH_RELAY_NETWORK, Vec::new());
        tech.define(TECH_LUMEN_CORE, vec![TECH_RELAY_NETWORK]);
        let mut structures = HashMap::new();
        for (index, _) in mission.relays.iter().enumerate() {
            structures.insert(
                StructureKind::Relay(index),
                StructureState {
                    kind: StructureKind::Relay(index),
                    health: 500.0,
                    max_health: 500.0,
                    build_progress: 0.0,
                    powered: false,
                },
            );
        }
        structures.insert(
            StructureKind::Fabricator,
            StructureState {
                kind: StructureKind::Fabricator,
                health: 800.0,
                max_health: 800.0,
                build_progress: 1.0,
                powered: true,
            },
        );
        if mission.reactor_position.is_some() {
            structures.insert(
                StructureKind::Reactor,
                StructureState {
                    kind: StructureKind::Reactor,
                    health: 1000.0,
                    max_health: 1000.0,
                    build_progress: 0.0,
                    powered: false,
                },
            );
        }
        let mut simulation = Self {
            world: RtsWorld::default(),
            kinds: HashMap::new(),
            identities: HashMap::new(),
            nav,
            player_paths: HashMap::new(),
            escort_unit: None,
            relays: mission
                .relays
                .iter()
                .map(|&position| Relay {
                    position,
                    progress: 0.0,
                    active: false,
                })
                .collect(),
            resources: ResourceBank::new(modifiers.starting_salvage),
            flux: 3,
            supply: SupplyLedger::new(12),
            tech,
            structures,
            terrain_zones: mission.terrain_zones.clone(),
            resource_objective: mission.resource_objective,
            resource_objective_target: mission
                .resource_objective
                .and_then(|objective| mission.salvage_nodes.get(objective.node_index).copied()),
            resource_objective_state: ResourceObjectiveState::default(),
            reactor_position: mission.reactor_position,
            enemy_resources: ResourceSet::new(0, 0),
            enemy_raid_count: 0,
            salvage_delivered: 0,
            production: ProductionQueue::new(5),
            structure_builds: BuildQueue::new(1),
            power,
            fabricator_position: mission.fabricator_position,
            rally_point: None,
            outcome: MissionOutcome::InProgress,
            ability_cooldowns: CooldownBook::default(),
            command_surges: CooldownBook::default(),
            scan_pulse: None,
            supply_module_progress: None,
            supply_module_level: 0,
            tick: 0,
            fixed_dt: 1.0 / DEFAULT_FIXED_TICK_HZ as f32,
            resource_tick: 0.0,
            modifiers,
            victory_condition: mission.victory,
            boss_reinforced: false,
            events: VecDeque::with_capacity(EVENT_LOG_CAPACITY),
            pending_events: VecDeque::with_capacity(EVENT_LOG_CAPACITY),
            path_advance_ids: Vec::with_capacity(PATH_ADVANCE_BUFFER_CAPACITY),
            attack_move_paths: HashSet::new(),
            combat_snapshot: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            combat_attacks: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            combat_resolver: RtsCombatResolver::new(),
            combat_detonations: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            combat_targets: HashMap::new(),
            attack_telegraph_timers: HashMap::new(),
            combat_target_updates: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            combat_telegraph_candidates: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            retreating_units: HashSet::new(),
            support_repairs: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            structure_repairs: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            enemy_income_tick: 0.0,
            enemy_raid_timer: FIRST_ENEMY_RAID_DELAY,
            enemy_telegraph_emitted: false,
            destroyed_by_kind: HashMap::new(),
        };
        for obstacle in &mission.obstacles {
            let _ = simulation.world.add_block_obstacle(*obstacle);
        }
        let _ = simulation.world.add_block_obstacle(Aabb::from_center_size(
            simulation.fabricator_position,
            Vec2::splat(StructureKind::FABRICATOR_RADIUS * 2.0),
        ));
        for relay in &mission.relays {
            let _ = simulation.world.add_block_obstacle(Aabb::from_center_size(
                *relay,
                Vec2::splat(StructureKind::RELAY_RADIUS * 2.0),
            ));
        }
        if let Some(reactor_position) = simulation.reactor_position {
            let _ = simulation.world.add_block_obstacle(Aabb::from_center_size(
                reactor_position,
                Vec2::splat(StructureKind::REACTOR_RADIUS * 2.0),
            ));
        }
        for spawn in &mission.player_spawns {
            let id = simulation.spawn(
                spawn.kind,
                PLAYER,
                spawn.position,
                spawn.health,
                spawn.speed,
                simulation.modifiers,
            );
            if let Some(callsign) = spawn.callsign {
                simulation.identities.insert(id, callsign);
            }
            if spawn.escort {
                simulation.escort_unit = Some(id);
            }
        }
        for spawn in &mission.enemy_spawns {
            simulation.spawn(
                spawn.kind,
                CHOIR,
                spawn.position,
                spawn.health,
                spawn.speed,
                simulation.modifiers,
            );
        }
        simulation
    }

    /// Converts Last Light's presentation/balance profile into the engine's
    /// renderer-independent combat contract. Last Light still authors armor
    /// as a small flat tuning value; the engine consumes a normalized
    /// reduction fraction, so this bridge keeps the existing balance readable
    /// while making range, damage type, and terrain resolution shared.
    fn engine_profile(kind: UnitKind, damage: f32) -> EngineCombatProfile {
        let profile = kind.combat();
        EngineCombatProfile::new(
            damage.max(0.0),
            profile.attack_period,
            profile.range,
            profile.damage_type,
            profile.armor_class,
        )
        .with_armor((profile.armor * 0.03).clamp(0.0, 0.5))
    }

    /// Return the full fire interval and its readable charge window. Keeping
    /// this beside the combat bridge makes target acquisition and recurring
    /// telegraphs share one cadence instead of slowly drifting apart.
    fn attack_timing(kind: UnitKind) -> (f32, f32) {
        let interval = kind
            .combat()
            .attack_period
            .max(ATTACK_TELEGRAPH_MIN_INTERVAL);
        let windup = (interval * 0.28).clamp(0.12, 0.22);
        (interval, windup)
    }

    fn engine_profile_for_unit(&self, id: UnitId) -> Option<EngineCombatProfile> {
        let unit = self.world.unit(id)?;
        let kind = self.kinds.get(&id).copied()?;
        let combat = kind.combat();
        let mut damage = combat.damage_per_second * combat.attack_period;
        if unit.faction == PLAYER {
            damage *= self.modifiers.player_damage_scale;
            if self.command_surge_remaining(id) > 0.0 {
                damage *= COMMAND_SURGE_DAMAGE_SCALE;
            }
        }
        Some(Self::engine_profile(kind, damage))
    }

    pub fn spawn(
        &mut self,
        kind: UnitKind,
        faction: FactionId,
        position: Vec2,
        health: f32,
        speed: f32,
        modifiers: SimulationModifiers,
    ) -> UnitId {
        let id = self.world.spawn(faction, position);
        let health = if faction == PLAYER {
            health * modifiers.player_health
        } else {
            health
        };
        let speed = if faction == PLAYER {
            speed * modifiers.player_speed
        } else {
            speed
        };
        if let Some(unit) = self.world.unit_mut(id) {
            unit.health = health;
            unit.max_health = health;
            unit.speed = speed;
            unit.radius = kind.scale() * 0.27;
            // Keep the shared engine profile populated even when a caller
            // drives the world directly (for example a headless playtest).
            // `update_combat` refreshes the pulse damage for variable-dt
            // callers before invoking the resolver.
            let combat = kind.combat();
            unit.combat =
                Self::engine_profile(kind, combat.damage_per_second * combat.attack_period);
            // Keep movement and combat on the same authored contract: an
            // explicit Attack order stops just inside the profile's range so
            // the squad forms a readable firing line instead of overlapping
            // its target.
            unit.engagement_range = (kind.combat().range * 0.96).max(0.0);
        }
        if faction == PLAYER {
            // Starting units and deployed production reserve the same ledger;
            // a failed queue is rejected before a unit can exceed supply.
            let _ = self.supply.try_add(kind.supply_cost());
        }
        self.kinds.insert(id, kind);
        id
    }

    pub fn callsign(&self, id: UnitId) -> Option<&'static str> {
        self.identities.get(&id).copied()
    }

    fn next_raid_kind(&self) -> UnitKind {
        if self.enemy_raid_count % 3 == 2 {
            UnitKind::BellMine
        } else {
            UnitKind::Needle
        }
    }

    fn raid_anchor(&self) -> Vec2 {
        self.relays
            .iter()
            .find(|relay| relay.active)
            .map(|relay| relay.position)
            .unwrap_or(self.fabricator_position)
    }

    fn raid_spawn_position(&self) -> Vec2 {
        let offset = if self.enemy_raid_count == 0 {
            // The first contact is deliberately visible on approach instead
            // of appearing on top of the player-facing relay.
            Vec2::new(640.0, -420.0)
        } else if self.enemy_raid_count.is_multiple_of(2) {
            Vec2::new(420.0, -280.0)
        } else {
            Vec2::new(-380.0, 240.0)
        };
        self.raid_anchor() + offset
    }

    /// Returns the next raid's phase and approach coordinates for compact HUD
    /// countdowns, minimap markers, and deterministic playtest assertions.
    pub fn raid_state(&self) -> RaidState {
        let funded = self.enemy_resources.primary >= 90;
        let phase = if self.enemy_raid_count == 0 && self.enemy_raid_timer > RAID_WARNING_WINDOW {
            RaidPhase::Teaching
        } else if self.enemy_raid_timer <= RAID_WARNING_WINDOW && funded {
            RaidPhase::Warning
        } else if !funded {
            RaidPhase::Banking
        } else {
            RaidPhase::Cooldown
        };
        RaidState {
            number: self.enemy_raid_count.saturating_add(1),
            phase,
            kind: self.next_raid_kind(),
            seconds_remaining: self.enemy_raid_timer.max(0.0),
            anchor: self.raid_anchor(),
            spawn_position: self.raid_spawn_position(),
            funded,
        }
    }

    /// The last target selected by combat resolution. Unlike an `Attack` order
    /// this also works for attack-move units whose target is discovered by the
    /// simulation each fixed tick.
    pub fn combat_target(&self, id: UnitId) -> Option<UnitId> {
        self.combat_targets.get(&id).copied()
    }

    /// Snapshot one unit's tactical state for the presentation layer.
    pub fn combat_contact(&self, id: UnitId) -> Option<CombatContact> {
        let unit = self.world.unit(id)?;
        let kind = self.kinds.get(&id).copied()?;
        let target = self.combat_target(id).or(match unit.order {
            UnitOrder::Attack(target) => Some(target),
            _ => None,
        });
        let state = if self.retreating_units.contains(&id) {
            CombatContactState::Retreating
        } else {
            match unit.order {
                UnitOrder::Attack(_) | UnitOrder::AttackMove(_) if target.is_some() => {
                    CombatContactState::Attacking
                }
                UnitOrder::Move(_) | UnitOrder::Patrol(_, _) | UnitOrder::Follow(_) => {
                    CombatContactState::Moving
                }
                UnitOrder::Hold => CombatContactState::Holding,
                _ => CombatContactState::Idle,
            }
        };
        Some(CombatContact {
            unit_id: id,
            kind,
            faction: unit.faction,
            position: unit.position,
            health_fraction: (unit.health / unit.max_health.max(1.0)).clamp(0.0, 1.0),
            state,
            target,
        })
    }

    /// Collects a value snapshot in stable world order. This is intentionally
    /// explicit (rather than exposing internal maps) so a HUD can render an
    /// overview without borrowing mutable simulation state.
    #[allow(dead_code)]
    pub fn combat_contacts(&self, faction: FactionId) -> Vec<CombatContact> {
        self.world
            .units()
            .iter()
            .filter(|unit| unit.faction == faction && unit.alive())
            .filter_map(|unit| self.combat_contact(unit.id))
            .collect()
    }

    pub fn select_all_player_units(&mut self) {
        self.world
            .select_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE), PLAYER, false);
    }

    pub fn select_player_kind(&mut self, kind: UnitKind) -> bool {
        let Some(position) = self.world.units().iter().find_map(|unit| {
            (unit.faction == PLAYER && unit.alive() && self.kinds.get(&unit.id) == Some(&kind))
                .then_some(unit.position)
        }) else {
            return false;
        };
        self.world.select_point(position, PLAYER, false);
        true
    }

    pub fn issue_move_order(&mut self, destination: Vec2) {
        let selected_ids = self.world.selection().ids().to_vec();
        self.world.issue_move(destination, 74.0);
        self.route_around_obstacles(&selected_ids);
    }

    pub fn queue_move_order(&mut self, destination: Vec2) {
        self.world.queue_move(destination, 74.0);
    }

    pub fn issue_attack_move_order(&mut self, destination: Vec2, append: bool) {
        self.world.issue_attack_move(destination, append);
        if !append {
            let ids = self.world.selection().ids().to_vec();
            self.route_around_obstacles(&ids);
        }
    }

    pub fn issue_patrol_order(&mut self, destination: Vec2, append: bool) {
        self.world.issue_patrol(destination, append);
    }

    /// Issues a path-aware move to one unit without disturbing squad selection.
    pub fn issue_unit_move(&mut self, id: UnitId, destination: Vec2) -> bool {
        let Some(unit) = self.world.unit_mut(id) else {
            return false;
        };
        if !unit.alive() {
            return false;
        }
        unit.order = UnitOrder::Move(destination);
        self.route_around_obstacles(&[id]);
        true
    }

    pub fn set_rally_point(&mut self, destination: Vec2) {
        self.rally_point = Some(destination);
    }

    pub fn issue_attack_kind(&mut self, kind: UnitKind) -> bool {
        let Some(target) = self.world.units().iter().find_map(|unit| {
            (unit.faction == CHOIR && unit.alive() && self.kinds.get(&unit.id) == Some(&kind))
                .then_some(unit.id)
        }) else {
            return false;
        };
        self.world.issue_attack(target);
        true
    }

    pub fn set_combat_scales(&mut self, damage: f32, damage_taken: f32) {
        self.modifiers.player_damage_scale = damage.max(0.0);
        self.modifiers.player_damage_taken_scale = damage_taken.max(0.0);
    }

    pub fn issue_selected_motion_profile(&mut self, profile: MotionProfile) -> usize {
        let selected = self.world.selection().ids().to_vec();
        self.world.issue_speed_scale(profile.speed_scale.max(0.0));
        self.world.issue_steering(profile.steering.max(0.0));
        self.world.issue_separation_profile(
            profile.separation_radius.max(0.0),
            profile.separation_strength.max(0.0),
        );
        selected.len()
    }

    pub fn add_circular_motion_obstacle(&mut self, center: Vec2, radius: f32) -> Option<BlockId> {
        let radius = radius.max(0.0);
        if !center.is_finite() || !radius.is_finite() {
            return None;
        }
        self.world
            .add_block_obstacle(Aabb::from_center_size(center, Vec2::splat(radius * 2.0)))
    }

    pub fn ability_for_kind(kind: UnitKind) -> Option<SpecialAbility> {
        match kind {
            UnitKind::Warden => Some(SpecialAbility::CommandSurge),
            UnitKind::Engineer => Some(SpecialAbility::EmergencyRepair),
            UnitKind::Surveyor => Some(SpecialAbility::ScanPulse),
            UnitKind::Needle | UnitKind::Canticle | UnitKind::BellMine => None,
        }
    }

    pub fn ability_cooldown(&self, id: UnitId) -> f32 {
        self.ability_cooldowns.remaining_seconds(id)
    }

    pub fn command_surge_remaining(&self, id: UnitId) -> f32 {
        self.command_surges.remaining_seconds(id)
    }

    /// Activates the selected unit's one signature action. The operation is
    /// intentionally simulation-owned: the same call is safe from a mouse
    /// click, a hotkey, or a future semantic command without duplicating
    /// balance rules in the renderer.
    pub fn activate_ability(&mut self, id: UnitId) -> Result<SpecialAbility, AbilityError> {
        let Some(unit) = self.world.unit(id) else {
            return Err(AbilityError::NotAvailable);
        };
        if unit.faction != PLAYER || !unit.alive() {
            return Err(AbilityError::NotAvailable);
        }
        let Some(kind) = self.kinds.get(&id).copied() else {
            return Err(AbilityError::NotAvailable);
        };
        let Some(ability) = Self::ability_for_kind(kind) else {
            return Err(AbilityError::NotAvailable);
        };
        let unit_position = unit.position;
        if self.ability_cooldown(id) > 0.0 {
            return Err(AbilityError::Cooldown);
        }

        match ability {
            SpecialAbility::CommandSurge => {
                self.command_surges.arm(id, 6.0);
                self.ability_cooldowns.arm(id, 18.0);
            }
            SpecialAbility::EmergencyRepair => {
                // Prefer a damaged Lantern, then a damaged structure in the
                // same support envelope. Both branches are deterministic and
                // make the Engineer's active job useful even when no relay is
                // currently being restored.
                let target = self
                    .world
                    .units()
                    .iter()
                    .filter(|candidate| {
                        candidate.faction == PLAYER
                            && candidate.alive()
                            && candidate.id != id
                            && candidate.health + f32::EPSILON < candidate.max_health
                            && candidate.position.distance(unit_position) <= 320.0
                    })
                    .min_by(|left, right| {
                        (left.health / left.max_health.max(1.0))
                            .total_cmp(&(right.health / right.max_health.max(1.0)))
                            .then_with(|| left.id.0.cmp(&right.id.0))
                    })
                    .map(|candidate| candidate.id);
                if let Some(target) = target {
                    if let Some(ally) = self.world.unit_mut(target) {
                        ally.health = (ally.health + 90.0).min(ally.max_health);
                    }
                } else {
                    let relay_positions: Vec<Vec2> =
                        self.relays.iter().map(|relay| relay.position).collect();
                    let fabricator_position = self.fabricator_position;
                    let reactor_position = self.reactor_position;
                    let structure_target = self
                        .structures
                        .iter()
                        .filter(|(_, structure)| {
                            structure.health + f32::EPSILON < structure.max_health
                        })
                        .filter_map(|(kind, _structure)| {
                            let position = match *kind {
                                StructureKind::Relay(index) => relay_positions.get(index).copied(),
                                StructureKind::Fabricator => Some(fabricator_position),
                                StructureKind::Reactor => reactor_position,
                            }?;
                            Some((*kind, position.distance(unit_position)))
                        })
                        .filter(|(_, distance)| *distance <= 320.0)
                        .min_by(|left, right| {
                            left.1.total_cmp(&right.1).then_with(|| {
                                let key = |kind: StructureKind| match kind {
                                    StructureKind::Relay(index) => index as u32,
                                    StructureKind::Fabricator => 100,
                                    StructureKind::Reactor => 101,
                                };
                                key(left.0).cmp(&key(right.0))
                            })
                        })
                        .map(|(kind, _)| kind);
                    let Some(structure_target) = structure_target else {
                        return Err(AbilityError::NoTarget);
                    };
                    if let Some(structure) = self.structures.get_mut(&structure_target) {
                        structure.health = (structure.health + 120.0).min(structure.max_health);
                    }
                }
                self.ability_cooldowns.arm(id, 20.0);
            }
            SpecialAbility::ScanPulse => {
                self.scan_pulse = Some((unit_position, 5.0));
                self.ability_cooldowns.arm(id, 16.0);
            }
        }
        self.record(SimulationEventKind::AbilityActivated {
            unit_id: id.0,
            ability,
        });
        Ok(ability)
    }

    fn update_ability_timers(&mut self, dt: f32) {
        self.ability_cooldowns.tick(dt);
        self.command_surges.tick(dt);
        if let Some((_, remaining)) = self.scan_pulse.as_mut() {
            *remaining = (*remaining - dt).max(0.0);
            if *remaining <= 0.0 {
                self.scan_pulse = None;
            }
        }
    }

    pub fn apply_environmental_damage(&mut self, target: UnitId, amount: f32) {
        let destroyed = if let Some(unit) = self.world.unit_mut(target) {
            let was_alive = unit.alive();
            unit.health = (unit.health - amount.max(0.0)).max(0.0);
            was_alive && !unit.alive()
        } else {
            return;
        };
        self.record(SimulationEventKind::DamageApplied { target: target.0 });
        if destroyed {
            self.release_supply_and_record(target);
            let kind = self.kinds[&target];
            *self.destroyed_by_kind.entry(kind).or_insert(0) += 1;
            self.record(SimulationEventKind::UnitDestroyed {
                unit_id: target.0,
                kind,
            });
        }
    }

    fn release_supply_and_record(&mut self, target: UnitId) {
        let Some(unit) = self.world.unit(target) else {
            return;
        };
        if unit.faction == PLAYER {
            if let Some(kind) = self.kinds.get(&target) {
                self.supply.release(kind.supply_cost());
            }
        }
    }

    pub fn route_around_obstacles(&mut self, selected_ids: &[UnitId]) {
        for &id in selected_ids {
            let Some(unit) = self.world.unit(id) else {
                continue;
            };
            let (destination, attack_move) = match unit.order {
                UnitOrder::Move(destination) => (destination, false),
                UnitOrder::AttackMove(destination) => (destination, true),
                _ => continue,
            };
            if !destination.is_finite() {
                continue;
            }
            if self.nav.segment_blocked(unit.position, destination) {
                let mut path: VecDeque<Vec2> =
                    self.nav.find_path(unit.position, destination).into();
                if let Some(first) = path.pop_front() {
                    if let Some(unit) = self.world.unit_mut(id) {
                        unit.order = if attack_move {
                            UnitOrder::AttackMove(first)
                        } else {
                            UnitOrder::Move(first)
                        };
                    }
                    if attack_move {
                        self.attack_move_paths.insert(id);
                    } else {
                        self.attack_move_paths.remove(&id);
                    }
                    self.player_paths.insert(id, path);
                    continue;
                }
            }
            self.player_paths.remove(&id);
            self.attack_move_paths.remove(&id);
        }
    }

    pub fn advance_player_paths(&mut self) {
        if self.player_paths.is_empty() {
            return;
        }
        // Reuse a simulation-owned scratch buffer. We cannot mutate a map
        // while iterating its keys, but the roster is bounded and this keeps
        // path following allocation-stable during the fixed update.
        let mut ids = std::mem::take(&mut self.path_advance_ids);
        ids.clear();
        ids.extend(self.player_paths.keys().copied());
        for &id in &ids {
            let Some(unit) = self.world.unit(id) else {
                self.player_paths.remove(&id);
                self.attack_move_paths.remove(&id);
                continue;
            };
            if !matches!(unit.order, UnitOrder::Idle) {
                continue;
            }
            let attack_move = self.attack_move_paths.contains(&id);
            let done = match self.player_paths.get_mut(&id) {
                Some(queue) => {
                    if let Some(next) = queue.pop_front() {
                        if let Some(unit) = self.world.unit_mut(id) {
                            unit.order = if attack_move {
                                UnitOrder::AttackMove(next)
                            } else {
                                UnitOrder::Move(next)
                            };
                        }
                    }
                    queue.is_empty()
                }
                None => true,
            };
            if done {
                self.player_paths.remove(&id);
                self.attack_move_paths.remove(&id);
                if self.world.start_next_queued_order(id) {
                    self.route_around_obstacles(&[id]);
                }
            }
        }
        self.path_advance_ids = ids;
    }

    #[cfg(test)]
    fn allocation_buffer_capacities(&self) -> (usize, usize, usize, usize) {
        (
            self.path_advance_ids.capacity(),
            self.combat_snapshot.capacity(),
            self.combat_attacks.capacity(),
            self.support_repairs.capacity(),
        )
    }

    pub fn selected_engineer_near(&self, position: Vec2) -> bool {
        self.world.selection().ids().iter().any(|id| {
            self.kinds.get(id) == Some(&UnitKind::Engineer)
                && self
                    .world
                    .unit(*id)
                    .is_some_and(|unit| unit.alive() && unit.position.distance(position) < 110.0)
        })
    }

    /// Returns whether any living Engineer is close enough to work a relay.
    ///
    /// Relay restoration is a persistent field job, not a presentation
    /// selection state. Keeping this query separate from
    /// [`Self::selected_engineer_near`] lets the HUD continue to explain a
    /// selected Engineer's nearby interaction while the fixed-step contract
    /// keeps progressing after the player switches to a Warden or Surveyor.
    pub fn engineer_near(&self, position: Vec2) -> bool {
        // Worst-case stand-off while working a structure: the footprint
        // circumradius (an Engineer can wedge against a footprint corner),
        // plus one nav cell of approach and snapping slack, with the strict
        // comparison absorbing float error. Keeps relay activation reachable
        // for every grid alignment now that structure footprints block
        // movement.
        const RELAY_WORK_RANGE: f32 =
            StructureKind::RELAY_RADIUS * std::f32::consts::SQRT_2 + NAV_CELL_SIZE;
        self.world.units().iter().any(|unit| {
            unit.faction == PLAYER
                && unit.alive()
                && self.kinds.get(&unit.id) == Some(&UnitKind::Engineer)
                && unit.position.distance(position) < RELAY_WORK_RANGE
        })
    }

    pub fn destroyed_count(&self, kind: UnitKind) -> u32 {
        self.destroyed_by_kind.get(&kind).copied().unwrap_or(0)
    }

    pub fn structure(&self, kind: StructureKind) -> Option<StructureState> {
        self.structures.get(&kind).copied()
    }

    /// Returns the authored resource objective and its resolved node
    /// position. Keeping the pair together prevents a HUD or replay tool from
    /// accidentally displaying a stale node index after a map expansion.
    pub fn resource_objective_contract(&self) -> Option<(ResourceObjective, Vec2)> {
        self.resource_objective.zip(self.resource_objective_target)
    }

    #[allow(dead_code)]
    pub fn resource_objective_state(&self) -> Option<ResourceObjectiveState> {
        self.resource_objective
            .is_some()
            .then_some(self.resource_objective_state)
    }

    fn update_resource_objective(&mut self, dt: f32) {
        let Some((objective, target)) = self.resource_objective_contract() else {
            return;
        };
        let worker_present = self.world.units().iter().any(|unit| {
            unit.faction == PLAYER
                && unit.alive()
                && self.kinds.get(&unit.id) == Some(&objective.worker_kind)
                && unit.position.distance(target) <= objective.worker_radius
        });
        let support_present = objective.support_kind.is_none_or(|support_kind| {
            self.world.units().iter().any(|unit| {
                unit.faction == PLAYER
                    && unit.alive()
                    && self.kinds.get(&unit.id) == Some(&support_kind)
                    && unit.position.distance(target) <= objective.support_radius
            })
        });
        let enemy_present = self.world.units().iter().any(|unit| {
            unit.faction == CHOIR
                && unit.alive()
                && unit.position.distance(target) <= objective.contest_radius
        });
        let _ = self.resource_objective_state.advance(
            objective,
            worker_present,
            support_present,
            enemy_present,
            dt,
        );
    }

    pub fn damage_structure(&mut self, kind: StructureKind, amount: f32) -> bool {
        let Some(structure) = self.structures.get_mut(&kind) else {
            return false;
        };
        let was_operational = structure.operational();
        structure.health = (structure.health - amount.max(0.0)).max(0.0);
        was_operational && !structure.operational()
    }

    fn structure_position(&self, kind: StructureKind) -> Option<Vec2> {
        match kind {
            StructureKind::Relay(index) => self.relays.get(index).map(|relay| relay.position),
            StructureKind::Fabricator => Some(self.fabricator_position),
            StructureKind::Reactor => self.reactor_position,
        }
    }

    pub fn credit_salvage(&mut self, amount: u32) {
        if amount == 0 {
            return;
        }
        self.resources.credit(amount);
        self.salvage_delivered = self.salvage_delivered.saturating_add(amount);
        self.record(SimulationEventKind::ResourcesDelivered {
            salvage: amount,
            flux: 0,
        });
    }

    pub fn credit_flux(&mut self, amount: u32) {
        if amount == 0 {
            return;
        }
        self.flux = self.flux.saturating_add(amount);
        self.record(SimulationEventKind::ResourcesDelivered {
            salvage: 0,
            flux: amount,
        });
    }

    pub fn activate_lumen_core(&mut self) -> bool {
        if self.relays.iter().all(|relay| relay.active) {
            let _ = self.tech.unlock(TECH_RELAY_NETWORK);
        }
        if !self.tech.is_unlocked(TECH_RELAY_NETWORK) || !self.resources.spend(90) {
            return false;
        }
        if !self.tech.is_unlocked(TECH_LUMEN_CORE) {
            let _ = self.tech.unlock(TECH_LUMEN_CORE);
        }
        true
    }

    pub fn queue_unit(&mut self, kind: UnitKind) -> Result<(), ProductionCommandError> {
        let friendly_count = self
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
            .count();
        if friendly_count + self.production.items().len() >= 12 {
            return Err(ProductionCommandError::UnitCap);
        }
        let supply_cost = kind.supply_cost();
        if self.supply.available() < supply_cost {
            return Err(ProductionCommandError::SupplyBlocked);
        }
        if !self.power.is_powered(FABRICATOR_NODE) {
            return Err(ProductionCommandError::FabricatorOffline);
        }
        let Some(mut recipe) = kind.recipe() else {
            return Err(ProductionCommandError::UnsupportedUnit);
        };
        let secondary_cost = kind.resource_cost().secondary;
        if self.flux < secondary_cost {
            return Err(ProductionCommandError::InsufficientFlux);
        }
        recipe.build_millis =
            (recipe.build_millis as f32 * self.modifiers.production_time_scale) as u32;
        self.production
            .enqueue_with_supply(recipe, &mut self.resources, &mut self.supply, supply_cost)
            .map_err(|error| match error {
                SupplyQueueError::InsufficientResources => {
                    ProductionCommandError::Queue(QueueError::InsufficientResources)
                }
                SupplyQueueError::InsufficientSupply => ProductionCommandError::SupplyBlocked,
                SupplyQueueError::Full => ProductionCommandError::Queue(QueueError::Full),
            })?;
        self.flux -= secondary_cost;
        self.record(SimulationEventKind::UnitQueued { kind });
        Ok(())
    }

    /// Cancels one queued Fabricator unit, returning a deterministic partial
    /// refund and releasing its reserved supply. The front job may be
    /// cancelled just like any other slot; removing a later slot leaves the
    /// active job and its progress untouched.
    pub fn cancel_queued_unit(
        &mut self,
        index: usize,
    ) -> Result<UnitQueueCancelReceipt, ProductionCancelCommandError> {
        if !self.power.is_powered(FABRICATOR_NODE) {
            return Err(ProductionCancelCommandError::FabricatorOffline);
        }
        let Some(item) = self.production.items().get(index).copied() else {
            return Err(ProductionCancelCommandError::InvalidIndex);
        };
        let Some(kind) = UnitKind::from_product(item.product) else {
            return Err(ProductionCancelCommandError::UnsupportedUnit);
        };
        let refunded_flux = Self::queue_cancel_refund(kind.resource_cost().secondary);
        let receipt = self
            .production
            .cancel_with_supply(
                index,
                &mut self.resources,
                &mut self.supply,
                QUEUE_CANCEL_REFUND_PERCENT,
            )
            .map_err(|error| match error {
                EngineProductionCancelError::InvalidIndex => {
                    ProductionCancelCommandError::InvalidIndex
                }
                EngineProductionCancelError::SupplyLedgerRequired => {
                    ProductionCancelCommandError::SupplyLedgerRequired
                }
            })?;
        self.flux = self.flux.saturating_add(refunded_flux);
        let result = UnitQueueCancelReceipt {
            kind,
            refunded_salvage: receipt.refunded_resources,
            refunded_flux,
            released_supply: receipt.released_supply,
        };
        Ok(result)
    }

    fn queue_cancel_refund(cost: u32) -> u32 {
        if cost == 0 {
            return 0;
        }
        // Flux is a one-unit secondary cost for Surveyors. Round up so a
        // cancellation never silently loses the entire strategic payment,
        // while keeping the refund deterministic for larger future costs.
        let numerator = u64::from(cost) * u64::from(QUEUE_CANCEL_REFUND_PERCENT) + 99;
        (numerator / 100).min(u64::from(u32::MAX)) as u32
    }

    pub fn queue_supply_module(&mut self) -> Result<(), StructureCommandError> {
        const COST: u32 = 100;
        const MAX_LEVEL: u8 = 3;
        if !self.power.is_powered(FABRICATOR_NODE) {
            return Err(StructureCommandError::FabricatorOffline);
        }
        if !self.structure_builds.is_empty() {
            return Err(StructureCommandError::Busy);
        }
        if self.supply_module_level >= MAX_LEVEL
            || self.supply.capacity() >= 12 + u32::from(MAX_LEVEL) * 4
        {
            return Err(StructureCommandError::Maxed);
        }
        if !self.resources.spend(COST) {
            return Err(StructureCommandError::InsufficientResources);
        }
        self.structure_builds
            .enqueue(BuildRecipe::new(SUPPLY_MODULE_BUILD_ID, 6.0))
            .expect("capacity check keeps the structure queue available");
        self.supply_module_progress = self
            .structure_builds
            .front()
            .map(|item| item.remaining_seconds);
        self.record(SimulationEventKind::StructureBuildQueued {
            structure: "supply_module".to_owned(),
        });
        Ok(())
    }

    pub fn supply_module_percent(&self) -> Option<u32> {
        self.supply_module_progress
            .map(|remaining| ((1.0 - remaining / 6.0).clamp(0.0, 1.0) * 100.0).round() as u32)
    }

    fn update_structure_builds(&mut self, dt: f32) {
        if !self.power.is_powered(FABRICATOR_NODE) {
            return;
        }
        let completed = self.structure_builds.update(dt);
        self.supply_module_progress = self
            .structure_builds
            .front()
            .map(|item| item.remaining_seconds);
        for build in completed {
            if build != SUPPLY_MODULE_BUILD_ID {
                continue;
            }
            self.supply_module_level = self.supply_module_level.saturating_add(1);
            self.supply
                .set_capacity((12 + u32::from(self.supply_module_level) * 4).min(24));
            self.record(SimulationEventKind::StructureBuildCompleted {
                structure: "supply_module".to_owned(),
            });
        }
    }

    #[cfg(test)]
    fn events(&self) -> &VecDeque<SimulationEvent> {
        &self.events
    }

    pub fn pop_pending_event(&mut self) -> Option<SimulationEvent> {
        self.pending_events.pop_front()
    }

    fn record(&mut self, kind: SimulationEventKind) {
        let event = SimulationEvent {
            tick: self.tick,
            kind,
        };
        if self.events.len() == EVENT_LOG_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event.clone());
        if self.pending_events.len() == EVENT_LOG_CAPACITY {
            self.pending_events.pop_front();
        }
        self.pending_events.push_back(event);
    }

    fn update_relays(&mut self, dt: f32) {
        for index in 0..self.relays.len() {
            if self.relays[index].active {
                continue;
            }
            let position = self.relays[index].position;
            if self.engineer_near(position) {
                self.relays[index].progress += dt.max(0.0) * self.modifiers.relay_restore_rate;
                if self.relays[index].progress >= 3.0 {
                    self.relays[index].progress = 3.0;
                    self.relays[index].active = true;
                    self.power.set_online(PowerNodeId(index as u16 + 1), true);
                    self.record(SimulationEventKind::RelayActivated { index });
                }
            }
            if let Some(structure) = self.structures.get_mut(&StructureKind::Relay(index)) {
                structure.build_progress = (self.relays[index].progress / 3.0).clamp(0.0, 1.0);
                structure.powered = self.relays[index].active;
            }
        }
        if self.relays.iter().all(|relay| relay.active) {
            let _ = self.tech.unlock(TECH_RELAY_NETWORK);
        }
        if let Some(structure) = self.structures.get_mut(&StructureKind::Reactor) {
            structure.build_progress = if self.tech.is_unlocked(TECH_RELAY_NETWORK) {
                1.0
            } else {
                0.0
            };
            structure.powered = self.tech.is_unlocked(TECH_RELAY_NETWORK);
        }
        if let Some(structure) = self.structures.get_mut(&StructureKind::Fabricator) {
            structure.powered = self.power.is_powered(FABRICATOR_NODE);
        }
    }

    fn update_economy(&mut self, dt: f32) {
        let active_relays = self.relays.iter().filter(|relay| relay.active).count() as f32;
        self.resource_tick +=
            dt.max(0.0) * active_relays * self.modifiers.relay_income_per_second as f32;
        let income = self.resource_tick.floor() as u32;
        if income > 0 {
            self.resources.credit(income);
            self.resource_tick -= income as f32;
            self.record(SimulationEventKind::ResourcesCredited { amount: income });
        }

        if self.power.is_powered(FABRICATOR_NODE) {
            for product in self.production.update(dt) {
                let Some(kind) = UnitKind::from_product(product) else {
                    continue;
                };
                let friendly_count = self
                    .world
                    .units()
                    .iter()
                    .filter(|unit| unit.faction == PLAYER && unit.alive())
                    .count();
                let offset = Vec2::new(80.0, (friendly_count % 3) as f32 * 70.0 - 70.0);
                let Some(stats) = kind.production_stats() else {
                    continue;
                };
                // Queue reservation becomes a live unit reservation at deployment.
                self.supply.release(kind.supply_cost());
                let id = self.spawn(
                    kind,
                    PLAYER,
                    self.rally_point.unwrap_or(self.fabricator_position) + offset,
                    stats.max_health,
                    stats.speed,
                    self.modifiers,
                );
                self.record(SimulationEventKind::UnitDeployed {
                    unit_id: id.0,
                    kind,
                });
            }
        }
        self.update_enemy_economy(dt);
    }

    fn update_enemy_economy(&mut self, dt: f32) {
        let relay_income = self.relays.iter().filter(|relay| relay.active).count() as f32;
        self.enemy_income_tick += dt.max(0.0) * (1.25 + relay_income * 0.55);
        let income = self.enemy_income_tick.floor() as u32;
        if income > 0 {
            self.enemy_resources.primary = self.enemy_resources.primary.saturating_add(income);
            self.enemy_income_tick -= income as f32;
        }
        self.enemy_raid_timer -= dt.max(0.0);
        let living_enemy_count = self
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == CHOIR && unit.alive())
            .count();
        if living_enemy_count >= 18 {
            self.enemy_raid_timer = 12.0;
            self.enemy_telegraph_emitted = false;
            return;
        }
        let funded = self.enemy_resources.primary >= 90;
        if self.enemy_raid_timer <= RAID_WARNING_WINDOW && funded && !self.enemy_telegraph_emitted {
            let state = self.raid_state();
            self.record(SimulationEventKind::EnemyRaidTelegraph {
                number: state.number,
                kind: state.kind,
                anchor_x: state.anchor.x,
                anchor_y: state.anchor.y,
                spawn_x: state.spawn_position.x,
                spawn_y: state.spawn_position.y,
                seconds_remaining: state.seconds_remaining,
            });
            self.enemy_telegraph_emitted = true;
        }
        if self.enemy_raid_timer > 0.0 || !funded {
            return;
        }
        let first_raid = self.enemy_raid_count == 0;
        let kind = self.next_raid_kind();
        let (health, speed) = match kind {
            UnitKind::Needle if first_raid => (82.0, 115.0),
            UnitKind::Needle => (95.0, 130.0),
            UnitKind::BellMine => (110.0, 80.0),
            _ => unreachable!("raid roster is enemy-only"),
        };
        // Keep the first contact intentionally light, then let the Choir's
        // economy create a visible difficulty curve. The cap prevents a long
        // mission from producing invulnerable stat bricks while still making
        // ignoring later raids increasingly expensive.
        let escalation = (self.enemy_raid_count as f32 * LATE_RAID_HEALTH_STEP).min(0.60);
        let speed_escalation = (self.enemy_raid_count as f32 * LATE_RAID_SPEED_STEP).min(0.20);
        let health = health * (1.0 + escalation);
        let speed = speed * (1.0 + speed_escalation);
        let id = self.spawn(
            kind,
            CHOIR,
            self.raid_spawn_position(),
            health,
            speed,
            self.modifiers,
        );
        if let Some((index, _)) = self
            .relays
            .iter()
            .enumerate()
            .find(|(_, relay)| relay.active)
        {
            let relay_damage = if first_raid {
                FIRST_RAID_RELAY_DAMAGE
            } else {
                STANDARD_RAID_RELAY_DAMAGE
                    + (self.enemy_raid_count as f32 * LATE_RAID_RELAY_DAMAGE_STEP).min(10.0)
            };
            let _ = self.damage_structure(StructureKind::Relay(index), relay_damage);
        }
        self.enemy_resources.primary -= 90;
        self.enemy_raid_count += 1;
        self.enemy_raid_timer = ENEMY_RAID_INTERVAL;
        self.enemy_telegraph_emitted = false;
        self.record(SimulationEventKind::EnemyRaidSpawned {
            unit_id: id.0,
            kind,
        });
    }

    fn apply_combat_damage(&mut self, attacker: UnitId, target: UnitId, amount: f32) {
        let Some(target_faction) = self.world.unit(target).map(|unit| unit.faction) else {
            return;
        };
        let amount = if target_faction == PLAYER {
            amount
                * self.modifiers.player_damage_taken_scale
                * if self.command_surge_remaining(target) > 0.0 {
                    COMMAND_SURGE_DAMAGE_TAKEN_SCALE
                } else {
                    1.0
                }
        } else {
            amount
        };
        let destroyed = if let Some(unit) = self.world.unit_mut(target) {
            let was_alive = unit.alive();
            unit.health = (unit.health - amount.max(0.0)).max(0.0);
            was_alive && !unit.alive()
        } else {
            false
        };
        self.record(SimulationEventKind::AttackLanded {
            attacker: attacker.0,
            target: target.0,
        });
        if destroyed {
            let Some(kind) = self.kinds.get(&target).copied() else {
                return;
            };
            self.release_supply_and_record(target);
            *self.destroyed_by_kind.entry(kind).or_insert(0) += 1;
            self.record(SimulationEventKind::UnitDestroyed {
                unit_id: target.0,
                kind,
            });
        }
    }

    fn update_combat(&mut self, dt: f32) {
        self.combat_snapshot.clear();
        self.combat_snapshot.extend(
            self.world
                .units()
                .iter()
                .map(|unit| (unit.id, unit.position, unit.alive())),
        );
        self.combat_attacks.clear();
        self.combat_detonations.clear();
        self.combat_target_updates.clear();
        self.combat_telegraph_candidates.clear();
        // Target acquisition is intentionally authored in Last Light (it
        // understands attack-move, hold-position, and Bell Mines), while the
        // actual ordinary weapon pulse is resolved by the engine contract.
        // A preview world lets us pass transient acquired Attack orders to the
        // resolver without mutating the player's visible order state.
        let mut engine_world = self.world.clone();
        for preview in engine_world.units_mut() {
            preview.order = UnitOrder::Idle;
            if let Some(kind) = self.kinds.get(&preview.id).copied() {
                let combat = kind.combat();
                preview.combat =
                    Self::engine_profile(kind, combat.damage_per_second * combat.attack_period);
            }
        }
        for unit in self.world.units() {
            if !unit.alive() {
                self.combat_target_updates.push((unit.id, None));
                continue;
            }
            let proximity_target = self
                .kinds
                .get(&unit.id)
                .and_then(|kind| kind.detonation())
                .and_then(|detonation| {
                    self.world
                        .units()
                        .iter()
                        .filter(|candidate| {
                            candidate.faction != unit.faction
                                && candidate.alive()
                                && unit.position.distance(candidate.position) <= detonation.radius
                        })
                        .min_by(|left, right| {
                            unit.position
                                .distance(left.position)
                                .total_cmp(&unit.position.distance(right.position))
                                .then_with(|| left.id.0.cmp(&right.id.0))
                        })
                        .map(|candidate| candidate.id)
                });
            let target = match unit.order {
                UnitOrder::Attack(target) => Some(target),
                UnitOrder::AttackMove(_) => self
                    .world
                    .units()
                    .iter()
                    .filter(|candidate| {
                        candidate.faction != unit.faction
                            && candidate.alive()
                            && unit.position.distance(candidate.position)
                                <= self.kinds[&unit.id].combat().range * 1.35
                    })
                    .min_by(|left, right| {
                        unit.position
                            .distance(left.position)
                            .total_cmp(&unit.position.distance(right.position))
                            .then_with(|| left.id.0.cmp(&right.id.0))
                    })
                    .map(|candidate| candidate.id),
                UnitOrder::Hold => {
                    // Hold Position is a defensive firing stance: acquire the
                    // nearest hostile only inside the authored weapon range,
                    // but never turn the order into a chase. This mirrors the
                    // player expectation of a StarCraft-style perimeter guard
                    // while keeping the movement integrator parked in place.
                    let range = self.kinds[&unit.id].combat().range;
                    self.world
                        .units()
                        .iter()
                        .filter(|candidate| {
                            candidate.faction != unit.faction
                                && candidate.alive()
                                && unit.position.distance(candidate.position) <= range
                        })
                        .min_by(|left, right| {
                            unit.position
                                .distance(left.position)
                                .total_cmp(&unit.position.distance(right.position))
                                .then_with(|| left.id.0.cmp(&right.id.0))
                        })
                        .map(|candidate| candidate.id)
                }
                // A Bell Mine is an area-denial unit, not a normal idle
                // attacker. It must arm from proximity alone so a player
                // cannot walk through an uncommanded trap safely.
                _ => proximity_target,
            };
            let target = target.filter(|target| {
                self.combat_snapshot
                    .iter()
                    .any(|(id, _, alive)| *id == *target && *alive)
                    && self
                        .world
                        .unit(*target)
                        .is_some_and(|candidate| candidate.faction != unit.faction)
            });
            self.combat_target_updates.push((unit.id, target));
            let Some(target) = target else { continue };
            let Some((_, target_position, true)) = self
                .combat_snapshot
                .iter()
                .find(|(id, _, _)| *id == target)
                .copied()
            else {
                continue;
            };
            let Some(profile) = self.kinds.get(&unit.id).map(|kind| kind.combat()) else {
                continue;
            };
            let attacker_elevation = self
                .terrain_zones
                .iter()
                .find(|zone| zone.contains(unit.position))
                .map(|zone| zone.elevation)
                .unwrap_or(profile.elevation);
            // Reach band: weapon range plus half the victim's footprint.
            // Motion pushes can settle an attacker slightly beyond the exact
            // center-to-center firing line against large targets (bosses,
            // structures), and demanding the ideal stand-off there deadlocks
            // the duel — the mover keeps re-driving into the push while the
            // resolver refuses to fire.
            let target_footprint = self
                .world
                .unit(target)
                .map(|candidate| candidate.radius.max(0.0))
                .unwrap_or(0.0);
            let weapon_reach = profile.range + target_footprint * 0.5;
            let target_distance = unit.position.distance(target_position);
            let inside_detonation_radius = self
                .kinds
                .get(&unit.id)
                .and_then(|kind| kind.detonation())
                .is_some_and(|detonation| target_distance <= detonation.radius);
            if target_distance < weapon_reach || inside_detonation_radius {
                self.combat_telegraph_candidates.push((unit.id, target));
                if let Some(detonation) =
                    self.kinds.get(&unit.id).and_then(|kind| kind.detonation())
                {
                    // Bell Mines are a one-shot area-denial answer to a
                    // clumped push. The explicit target must be in trigger
                    // range, then every hostile unit inside the blast radius
                    // receives one deterministic burst before the mine dies.
                    if !self.combat_detonations.contains(&unit.id) {
                        self.combat_detonations.push(unit.id);
                    }
                    for candidate in self.world.units() {
                        if !candidate.alive()
                            || candidate.faction == unit.faction
                            || candidate.position.distance(unit.position) > detonation.radius
                        {
                            continue;
                        }
                        let target_kind = self
                            .kinds
                            .get(&candidate.id)
                            .copied()
                            .unwrap_or(UnitKind::Warden);
                        let target_profile = target_kind.combat();
                        let target_terrain_scale = self
                            .terrain_zones
                            .iter()
                            .find(|zone| zone.contains(candidate.position))
                            .map(|zone| zone.damage_multiplier(attacker_elevation))
                            .unwrap_or(1.0);
                        let amount = (detonation.damage
                            * profile.damage_type.multiplier(target_profile.armor_class)
                            * target_terrain_scale
                            - target_profile.armor * 0.45)
                            .max(0.0)
                            * if unit.faction == PLAYER {
                                self.modifiers.player_damage_scale
                                    * if self.command_surge_remaining(unit.id) > 0.0 {
                                        COMMAND_SURGE_DAMAGE_SCALE
                                    } else {
                                        1.0
                                    }
                            } else {
                                1.0
                            };
                        self.combat_attacks.push((unit.id, candidate.id, amount));
                    }
                } else if let (Some(preview), Some(mut engine_profile)) = (
                    engine_world.unit_mut(unit.id),
                    self.engine_profile_for_unit(unit.id),
                ) {
                    // Mirror the reach band into the engine seam so the
                    // resolver accepts attacks from the same displaced
                    // stand-offs the sim-level gate just approved.
                    engine_profile.range += target_footprint * 0.5;
                    preview.combat = engine_profile;
                    preview.order = UnitOrder::Attack(target);
                }
            }
        }

        // Publish target transitions before hit events. A presentation layer
        // can attach a bracket immediately, then animate the later telegraph
        // and damage pulse without guessing from health deltas.
        let mut target_updates = std::mem::take(&mut self.combat_target_updates);
        for (attacker, target) in target_updates.drain(..) {
            let previous = self.combat_targets.get(&attacker).copied();
            if let Some(target) = target {
                if previous != Some(target) {
                    self.record(SimulationEventKind::TargetAcquired {
                        attacker: attacker.0,
                        target: target.0,
                    });
                    let kind = self
                        .kinds
                        .get(&attacker)
                        .copied()
                        .unwrap_or(UnitKind::Needle);
                    let (interval, windup) = Self::attack_timing(kind);
                    self.attack_telegraph_timers
                        .insert(attacker, (interval - windup).max(0.0));
                }
                self.combat_targets.insert(attacker, target);
            } else {
                if previous.is_some() {
                    self.record(SimulationEventKind::TargetLost {
                        attacker: attacker.0,
                    });
                }
                self.attack_telegraph_timers.remove(&attacker);
                self.combat_targets.remove(&attacker);
            }
        }
        self.combat_target_updates = target_updates;

        // Telegraphs follow each weapon's authored cadence. This lets a fast
        // Needle and a heavy Canticle read differently while preserving one
        // deterministic event path for native and browser presentation.
        let mut telegraph_candidates = std::mem::take(&mut self.combat_telegraph_candidates);
        for (attacker, target) in telegraph_candidates.drain(..) {
            let kind = self
                .kinds
                .get(&attacker)
                .copied()
                .unwrap_or(UnitKind::Needle);
            let (interval, windup_seconds) = Self::attack_timing(kind);
            let should_telegraph = {
                let timer = self.attack_telegraph_timers.entry(attacker).or_insert(0.0);
                *timer -= dt.max(0.0);
                if *timer <= 0.0 {
                    *timer = interval;
                    true
                } else {
                    false
                }
            };
            if should_telegraph {
                self.record(SimulationEventKind::AttackTelegraph {
                    attacker: attacker.0,
                    target: target.0,
                    windup_seconds,
                });
            }
        }
        self.combat_telegraph_candidates = telegraph_candidates;

        // Resolve ordinary attacks through the shared engine seam. Bell Mine
        // damage remains in `combat_attacks` because it is an authored
        // one-shot AoE rather than a normal weapon pulse.
        let engine_events = self
            .combat_resolver
            .update(&mut engine_world, dt, &self.terrain_zones);
        for event in engine_events {
            self.apply_combat_damage(event.attacker, event.target, event.damage);
        }

        for index in 0..self.combat_attacks.len() {
            let (attacker, target, amount) = self.combat_attacks[index];
            self.apply_combat_damage(attacker, target, amount);
        }

        let mut detonations = std::mem::take(&mut self.combat_detonations);
        for mine in detonations.drain(..) {
            let kind = self.kinds[&mine];
            self.record(SimulationEventKind::UnitDetonated {
                unit_id: mine.0,
                kind,
            });
            let destroyed = if let Some(unit) = self.world.unit_mut(mine) {
                let was_alive = unit.alive();
                unit.health = 0.0;
                was_alive && !unit.alive()
            } else {
                false
            };
            if destroyed {
                self.release_supply_and_record(mine);
                *self.destroyed_by_kind.entry(kind).or_insert(0) += 1;
                self.record(SimulationEventKind::UnitDestroyed {
                    unit_id: mine.0,
                    kind,
                });
            }
        }
        self.combat_detonations = detonations;
    }

    fn update_retreat_markers(&mut self) {
        let mut observed = HashSet::new();
        let mut entered = Vec::new();
        for unit in self.world.units() {
            let retreating = unit.faction == CHOIR
                && unit.alive()
                && unit.health / unit.max_health.max(1.0) <= ENEMY_RETREAT_HEALTH_FRACTION
                && matches!(unit.order, UnitOrder::Move(_));
            if retreating {
                observed.insert(unit.id);
                if !self.retreating_units.contains(&unit.id) {
                    entered.push((unit.id, self.kinds.get(&unit.id).copied()));
                }
            }
        }
        entered.sort_by_key(|(id, _)| id.0);
        for (id, kind) in entered {
            if let Some(kind) = kind {
                self.record(SimulationEventKind::UnitRetreating {
                    unit_id: id.0,
                    kind,
                });
            }
        }

        let mut exited: Vec<UnitId> = self
            .retreating_units
            .iter()
            .copied()
            .filter(|id| !observed.contains(id))
            .collect();
        exited.sort_by_key(|id| id.0);
        for id in exited {
            let recovered = self.world.unit(id).is_some_and(|unit| unit.alive());
            if recovered {
                if let Some(kind) = self.kinds.get(&id).copied() {
                    self.record(SimulationEventKind::UnitRecovered {
                        unit_id: id.0,
                        kind,
                    });
                }
            }
        }
        self.retreating_units = observed;
    }

    fn update_engineer_repairs(&mut self, dt: f32) {
        const REPAIR_RANGE: f32 = 145.0;
        const REPAIR_PER_SECOND: f32 = 12.0;
        self.support_repairs.clear();
        self.structure_repairs.clear();
        for engineer in self.world.units().iter().filter(|unit| {
            unit.faction == PLAYER
                && unit.alive()
                && self.kinds.get(&unit.id) == Some(&UnitKind::Engineer)
        }) {
            // An Engineer has one repair beam. Prefer a critically damaged
            // field unit; only when the squad is healthy does the beam fall
            // back to the nearest damaged structure. This keeps the support
            // job legible and prevents one Engineer from repairing two assets
            // at full rate at the same time.
            if let Some(target) =
                self.world
                    .most_damaged_ally_in_range(engineer.id, PLAYER, REPAIR_RANGE)
            {
                self.support_repairs
                    .push((engineer.id, target, REPAIR_PER_SECOND * dt.max(0.0)));
            } else if let Some((kind, _)) = self
                .structures
                .iter()
                .filter(|(_, structure)| structure.health < structure.max_health)
                .filter_map(|(kind, _structure)| {
                    self.structure_position(*kind)
                        .map(|position| (*kind, position.distance(engineer.position)))
                })
                .filter(|(_, distance)| *distance <= REPAIR_RANGE * 1.35)
                .min_by(|left, right| left.1.total_cmp(&right.1))
            {
                self.structure_repairs
                    .push((engineer.id, kind, REPAIR_PER_SECOND * dt.max(0.0)));
            }
        }
        for index in 0..self.support_repairs.len() {
            let (engineer, target, amount) = self.support_repairs[index];
            let repaired = if let Some(unit) = self.world.unit_mut(target) {
                unit.health = (unit.health + amount).min(unit.max_health);
                true
            } else {
                false
            };
            if repaired {
                self.record(SimulationEventKind::UnitRepaired {
                    engineer: engineer.0,
                    target: target.0,
                });
            }
        }
        for index in 0..self.structure_repairs.len() {
            let (_, kind, amount) = self.structure_repairs[index];
            if let Some(structure) = self.structures.get_mut(&kind) {
                structure.health = (structure.health + amount).min(structure.max_health);
                self.record(SimulationEventKind::StructureRepaired {
                    structure: format!("{kind:?}"),
                });
            }
        }
    }

    fn update_boss_phase(&mut self) {
        if self.boss_reinforced {
            return;
        }
        let position = self.world.units().iter().find_map(|unit| {
            (unit.faction == CHOIR
                && unit.alive()
                && self.kinds.get(&unit.id) == Some(&UnitKind::Canticle)
                && unit.health / unit.max_health.max(1.0) <= 0.5)
                .then_some(unit.position)
        });
        let Some(position) = position else {
            return;
        };
        self.boss_reinforced = true;
        for offset in [Vec2::new(-90.0, 40.0), Vec2::new(90.0, -40.0)] {
            let id = self.spawn(
                UnitKind::Needle,
                CHOIR,
                position + offset,
                90.0,
                125.0,
                self.modifiers,
            );
            self.record(SimulationEventKind::UnitSpawned {
                unit_id: id.0,
                kind: UnitKind::Needle,
            });
        }
        self.record(SimulationEventKind::BossReinforced);
    }

    fn evaluate_outcome(&mut self) {
        if self.outcome != MissionOutcome::InProgress {
            return;
        }
        let friendlies_alive = self
            .world
            .units()
            .iter()
            .any(|unit| unit.faction == PLAYER && unit.alive());
        let escort_alive = self
            .escort_unit
            .and_then(|id| self.world.unit(id))
            .is_some_and(|unit| unit.alive());
        let defeat = !friendlies_alive
            || (matches!(
                self.victory_condition,
                VictoryCondition::EscortToExtraction { .. }
            ) && !escort_alive);
        let victory = match self.victory_condition {
            VictoryCondition::RestoreRelaysAndDefeatBoss { boss_kind } => {
                let boss_alive = self.world.units().iter().any(|unit| {
                    unit.faction == CHOIR
                        && unit.alive()
                        && self.kinds.get(&unit.id) == Some(&boss_kind)
                });
                self.relays.iter().all(|relay| relay.active) && !boss_alive
            }
            VictoryCondition::EscortToExtraction { point, radius } => self
                .escort_unit
                .and_then(|id| self.world.unit(id))
                .is_some_and(|unit| unit.alive() && unit.position.distance(point) <= radius),
        };
        if defeat {
            self.outcome = MissionOutcome::Defeat;
            self.record(SimulationEventKind::MissionDefeat);
        } else if victory {
            self.outcome = MissionOutcome::Victory;
            self.record(SimulationEventKind::MissionVictory);
        }
    }

    pub fn fixed_step_with_dt(&mut self, dt: f32) {
        self.update_ability_timers(dt);
        self.world.update(dt);
        self.advance_player_paths();
        self.update_resource_objective(dt);
        self.update_relays(dt);
        self.update_structure_builds(dt);
        self.update_economy(dt);
        self.update_engineer_repairs(dt);
        self.update_combat(dt);
        self.update_retreat_markers();
        self.update_boss_phase();
        self.evaluate_outcome();
        self.tick = self.tick.saturating_add(1);
    }

    fn command_destination(command: &SemanticCommand) -> Result<Vec2, String> {
        let coordinates = command
            .payload
            .as_array()
            .filter(|coordinates| coordinates.len() == 2)
            .ok_or_else(|| "move payload must be [x, y]".to_owned())?;
        let x = coordinates[0]
            .as_f64()
            .ok_or_else(|| "move x must be numeric".to_owned())? as f32;
        let y = coordinates[1]
            .as_f64()
            .ok_or_else(|| "move y must be numeric".to_owned())? as f32;
        let destination = Vec2::new(x, y);
        if !destination.is_finite() {
            return Err("move destination must be finite".to_owned());
        }
        Ok(destination)
    }

    fn command_kind(command: &SemanticCommand) -> Result<UnitKind, String> {
        let label = command
            .payload
            .as_str()
            .ok_or_else(|| "unit payload must be a string".to_owned())?;
        match label {
            "warden" => Ok(UnitKind::Warden),
            "engineer" => Ok(UnitKind::Engineer),
            "surveyor" => Ok(UnitKind::Surveyor),
            "needle" => Ok(UnitKind::Needle),
            "canticle" => Ok(UnitKind::Canticle),
            "bell_mine" => Ok(UnitKind::BellMine),
            _ => Err(format!("unknown unit kind {label}")),
        }
    }
}

impl DeterministicSimulation for MissionSimulation {
    fn apply_command(&mut self, command: &SemanticCommand) -> Result<(), String> {
        match command.action.as_str() {
            SELECT_ALL_ACTION => {
                self.select_all_player_units();
            }
            SELECT_KIND_ACTION => {
                let kind = Self::command_kind(command)?;
                if !self.select_player_kind(kind) {
                    return Err(format!("no living {} available", kind.label()));
                }
            }
            MOVE_SELECTED_ACTION if !self.world.selection().ids().is_empty() => {
                self.issue_move_order(Self::command_destination(command)?);
            }
            MOVE_SELECTED_ACTION => {
                return Err("move requires a selected Lantern unit".to_owned());
            }
            ATTACK_MOVE_SELECTED_ACTION if !self.world.selection().ids().is_empty() => {
                self.issue_attack_move_order(Self::command_destination(command)?, false);
            }
            ATTACK_MOVE_SELECTED_ACTION => {
                return Err("attack-move requires a selected Lantern unit".to_owned());
            }
            PATROL_SELECTED_ACTION if !self.world.selection().ids().is_empty() => {
                self.issue_patrol_order(Self::command_destination(command)?, false);
            }
            PATROL_SELECTED_ACTION => {
                return Err("patrol requires a selected Lantern unit".to_owned());
            }
            QUEUE_UNIT_ACTION => self
                .queue_unit(Self::command_kind(command)?)
                .map_err(|error| format!("could not queue unit: {error:?}"))?,
            ATTACK_KIND_ACTION if !self.world.selection().ids().is_empty() => {
                let kind = Self::command_kind(command)?;
                if !self.issue_attack_kind(kind) {
                    return Err(format!("no living {} target available", kind.label()));
                }
            }
            ATTACK_KIND_ACTION => {
                return Err("attack requires a selected Lantern unit".to_owned());
            }
            action => return Err(format!("unknown Last Light action {action}")),
        }
        self.record(SimulationEventKind::CommandAccepted {
            action: command.action.clone(),
        });
        Ok(())
    }

    fn fixed_step(&mut self) {
        self.fixed_step_with_dt(self.fixed_dt);
    }

    fn state_hash(&self) -> StateHash {
        let mut hasher = StableStateHasher::new();
        hasher.write_u64(self.tick);
        hasher.write_u64(match self.outcome {
            MissionOutcome::InProgress => 0,
            MissionOutcome::Victory => 1,
            MissionOutcome::Defeat => 2,
        });
        hasher.write_bool(self.boss_reinforced);
        hasher.write_u64(self.world.units().len() as u64);
        for unit in self.world.units() {
            hasher.write_u64(u64::from(unit.id.0));
            hasher.write_u64(u64::from(unit.faction.0));
            hasher.write_u64(u64::from(self.kinds[&unit.id].atlas_frame()));
            for value in [
                unit.position.x,
                unit.position.y,
                unit.velocity.x,
                unit.velocity.y,
                unit.health,
            ] {
                hasher.write_u64(u64::from(value.to_bits()));
            }
            if let Some(callsign) = self.callsign(unit.id) {
                hasher.write_bool(true);
                hasher.write_bytes(callsign.as_bytes());
            } else {
                hasher.write_bool(false);
            }
            match unit.order {
                UnitOrder::Idle => hasher.write_u64(0),
                UnitOrder::Move(position) => {
                    hasher.write_u64(1);
                    hasher.write_u64(u64::from(position.x.to_bits()));
                    hasher.write_u64(u64::from(position.y.to_bits()));
                }
                UnitOrder::AttackMove(position) => {
                    hasher.write_u64(5);
                    hasher.write_u64(u64::from(position.x.to_bits()));
                    hasher.write_u64(u64::from(position.y.to_bits()));
                }
                UnitOrder::Attack(target) => {
                    hasher.write_u64(2);
                    hasher.write_u64(u64::from(target.0));
                }
                UnitOrder::Interact(position) => {
                    hasher.write_u64(3);
                    hasher.write_u64(u64::from(position.x.to_bits()));
                    hasher.write_u64(u64::from(position.y.to_bits()));
                }
                UnitOrder::Patrol(first, second) => {
                    hasher.write_u64(6);
                    for position in [first, second] {
                        hasher.write_u64(u64::from(position.x.to_bits()));
                        hasher.write_u64(u64::from(position.y.to_bits()));
                    }
                }
                UnitOrder::Follow(target) => {
                    hasher.write_u64(7);
                    hasher.write_u64(u64::from(target.0));
                }
                UnitOrder::Hold => hasher.write_u64(4),
            }
            hasher.write_u64(unit.queued_orders.len() as u64);
            for order in &unit.queued_orders {
                match *order {
                    UnitOrder::Idle => hasher.write_u64(0),
                    UnitOrder::Move(position) => {
                        hasher.write_u64(1);
                        hasher.write_u64(u64::from(position.x.to_bits()));
                        hasher.write_u64(u64::from(position.y.to_bits()));
                    }
                    UnitOrder::Attack(target) => {
                        hasher.write_u64(2);
                        hasher.write_u64(u64::from(target.0));
                    }
                    UnitOrder::Interact(position) => {
                        hasher.write_u64(3);
                        hasher.write_u64(u64::from(position.x.to_bits()));
                        hasher.write_u64(u64::from(position.y.to_bits()));
                    }
                    UnitOrder::Hold => hasher.write_u64(4),
                    UnitOrder::AttackMove(position) => {
                        hasher.write_u64(5);
                        hasher.write_u64(u64::from(position.x.to_bits()));
                        hasher.write_u64(u64::from(position.y.to_bits()));
                    }
                    UnitOrder::Patrol(first, second) => {
                        hasher.write_u64(6);
                        for position in [first, second] {
                            hasher.write_u64(u64::from(position.x.to_bits()));
                            hasher.write_u64(u64::from(position.y.to_bits()));
                        }
                    }
                    UnitOrder::Follow(target) => {
                        hasher.write_u64(7);
                        hasher.write_u64(u64::from(target.0));
                    }
                }
            }
            hasher.write_u64(u64::from(self.ability_cooldown(unit.id).to_bits()));
            hasher.write_u64(u64::from(self.command_surge_remaining(unit.id).to_bits()));
            hasher.write_u64(u64::from(self.combat_resolver.cooldown(unit.id).to_bits()));
        }
        let mut combat_targets: Vec<_> = self.combat_targets.iter().collect();
        combat_targets.sort_by_key(|(attacker, _)| attacker.0);
        hasher.write_u64(combat_targets.len() as u64);
        for (attacker, target) in combat_targets {
            hasher.write_u64(u64::from(attacker.0));
            hasher.write_u64(u64::from(target.0));
        }
        let mut telegraph_timers: Vec<_> = self.attack_telegraph_timers.iter().collect();
        telegraph_timers.sort_by_key(|(id, _)| id.0);
        hasher.write_u64(telegraph_timers.len() as u64);
        for (id, remaining) in telegraph_timers {
            hasher.write_u64(u64::from(id.0));
            hasher.write_u64(u64::from(remaining.to_bits()));
        }
        let mut retreating_ids: Vec<_> = self.retreating_units.iter().copied().collect();
        retreating_ids.sort_by_key(|id| id.0);
        hasher.write_u64(retreating_ids.len() as u64);
        for id in retreating_ids {
            hasher.write_u64(u64::from(id.0));
        }
        hasher.write_u64(self.world.selection().ids().len() as u64);
        for id in self.world.selection().ids() {
            hasher.write_u64(u64::from(id.0));
        }
        hasher.write_u64(u64::from(self.resources.amount()));
        hasher.write_u64(u64::from(self.flux));
        hasher.write_u64(u64::from(self.supply.used()));
        hasher.write_u64(u64::from(self.supply.capacity()));
        hasher.write_u64(self.tech.unlocked().len() as u64);
        for tech in self.tech.unlocked() {
            hasher.write_u64(u64::from(tech.0));
        }
        hasher.write_u64(u64::from(self.enemy_resources.primary));
        hasher.write_u64(u64::from(self.enemy_resources.secondary));
        hasher.write_u64(u64::from(self.enemy_raid_count));
        hasher.write_u64(u64::from(self.salvage_delivered));
        hasher.write_u64(u64::from(self.enemy_raid_timer.to_bits()));
        hasher.write_bool(self.enemy_telegraph_emitted);
        if let Some((position, remaining)) = self.scan_pulse {
            hasher.write_bool(true);
            hasher.write_u64(u64::from(position.x.to_bits()));
            hasher.write_u64(u64::from(position.y.to_bits()));
            hasher.write_u64(u64::from(remaining.to_bits()));
        } else {
            hasher.write_bool(false);
        }
        hasher.write_u64(u64::from(self.supply_module_level));
        if let Some(remaining) = self.supply_module_progress {
            hasher.write_bool(true);
            hasher.write_u64(u64::from(remaining.to_bits()));
        } else {
            hasher.write_bool(false);
        }
        for kind in [
            UnitKind::Warden,
            UnitKind::Engineer,
            UnitKind::Surveyor,
            UnitKind::Needle,
            UnitKind::Canticle,
            UnitKind::BellMine,
        ] {
            hasher.write_u64(u64::from(self.destroyed_count(kind)));
        }
        hasher.write_u64(u64::from(self.resource_tick.to_bits()));
        if let Some(rally) = self.rally_point {
            hasher.write_bool(true);
            hasher.write_u64(u64::from(rally.x.to_bits()));
            hasher.write_u64(u64::from(rally.y.to_bits()));
        } else {
            hasher.write_bool(false);
        }
        if let Some((objective, target)) = self.resource_objective_contract() {
            hasher.write_bool(true);
            hasher.write_u64(objective.node_index as u64);
            hasher.write_u64(u64::from(objective.worker_kind.atlas_frame()));
            if let Some(support_kind) = objective.support_kind {
                hasher.write_bool(true);
                hasher.write_u64(u64::from(support_kind.atlas_frame()));
            } else {
                hasher.write_bool(false);
            }
            hasher.write_u64(u64::from(target.x.to_bits()));
            hasher.write_u64(u64::from(target.y.to_bits()));
            for value in [
                objective.worker_radius,
                objective.support_radius,
                objective.contest_radius,
                objective.required_seconds,
                self.resource_objective_state.progress_seconds,
                self.resource_objective_state.contested_seconds,
            ] {
                hasher.write_u64(u64::from(value.to_bits()));
            }
            hasher.write_bool(self.resource_objective_state.contested);
            hasher.write_bool(self.resource_objective_state.completed);
        } else {
            hasher.write_bool(false);
        }
        hasher.write_u64(self.production.items().len() as u64);
        for item in self.production.items() {
            hasher.write_u64(u64::from(item.product.0));
            hasher.write_u64(u64::from(item.remaining_seconds.to_bits()));
            hasher.write_u64(u64::from(item.total_seconds.to_bits()));
        }
        hasher.write_u64(self.structure_builds.items().len() as u64);
        for item in self.structure_builds.items() {
            hasher.write_u64(u64::from(item.build.0));
            hasher.write_u64(u64::from(item.remaining_seconds.to_bits()));
            hasher.write_u64(u64::from(item.total_seconds.to_bits()));
        }
        for relay in &self.relays {
            hasher.write_u64(u64::from(relay.progress.to_bits()));
            hasher.write_bool(relay.active);
        }
        let mut structure_order: Vec<_> =
            (0..self.relays.len()).map(StructureKind::Relay).collect();
        structure_order.push(StructureKind::Fabricator);
        if self.structures.contains_key(&StructureKind::Reactor) {
            structure_order.push(StructureKind::Reactor);
        }
        for kind in structure_order {
            if let Some(structure) = self.structures.get(&kind) {
                hasher.write_u64(match kind {
                    StructureKind::Relay(index) => u64::from(index as u16),
                    StructureKind::Fabricator => 100,
                    StructureKind::Reactor => 101,
                });
                hasher.write_u64(u64::from(structure.health.to_bits()));
                hasher.write_u64(u64::from(structure.build_progress.to_bits()));
                hasher.write_bool(structure.powered);
            }
        }
        hasher.write_u64(self.events.len() as u64);
        for event in &self.events {
            hasher.write_u64(event.tick);
            let encoded = serde_json::to_vec(&event.kind)
                .expect("simulation events contain serializable deterministic fields");
            hasher.write_bytes(&encoded);
        }
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use aurora_engine::{run_trace, AuroraTrace, SemanticCommand};

    use super::*;

    fn reclaim_truth_trace() -> AuroraTrace {
        AuroraTrace::from_json(include_str!(
            "../../../playtests/last_light/reclaim_reactor_truth.aurora-trace"
        ))
        .expect("checked-in Reclaim trace must remain valid")
    }

    #[test]
    fn reclaim_truth_trace_replays_through_victory_with_the_same_hash() {
        let mission = crate::missions::reclaim_the_reactor();
        let trace = reclaim_truth_trace();
        let mut first = MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let mut second = MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let first_allocation_budget = first.allocation_buffer_capacities();
        let second_allocation_budget = second.allocation_buffer_capacities();

        let first_report = run_trace(&mut first, &trace).unwrap();
        let second_report = run_trace(&mut second, &trace).unwrap();

        assert_eq!(
            first_report.final_state_hash,
            second_report.final_state_hash
        );
        assert_eq!(first_report.commands_applied, 8);
        assert_eq!(
            first.allocation_buffer_capacities(),
            first_allocation_budget
        );
        assert_eq!(
            second.allocation_buffer_capacities(),
            second_allocation_budget
        );
        assert!(first_allocation_budget.0 >= PATH_ADVANCE_BUFFER_CAPACITY);
        assert!(first_allocation_budget.1 >= COMBAT_BUFFER_CAPACITY);
        assert!(first_allocation_budget.2 >= COMBAT_BUFFER_CAPACITY);
        assert!(first_allocation_budget.3 >= COMBAT_BUFFER_CAPACITY);
        assert!(first.relays.iter().all(|relay| relay.active));
        assert_eq!(first.outcome, MissionOutcome::Victory);
        assert_eq!(
            first
                .world
                .units()
                .iter()
                .filter(|unit| {
                    unit.faction == PLAYER && first.kinds.get(&unit.id) == Some(&UnitKind::Warden)
                })
                .count(),
            2,
            "the trace must deploy exactly one additional Warden"
        );
        assert!(first.events().len() <= EVENT_LOG_CAPACITY);
        assert!(first
            .events()
            .iter()
            .collect::<Vec<_>>()
            .windows(2)
            .all(|events| { events[0].tick <= events[1].tick }));
        assert!(first.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::UnitDestroyed {
                kind: UnitKind::Canticle,
                ..
            }
        )));
        assert!(first
            .events()
            .iter()
            .any(|event| matches!(event.kind, SimulationEventKind::MissionVictory)));
    }

    #[test]
    fn reclaim_trace_emits_relay_resource_queue_and_deploy_events() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut trace = reclaim_truth_trace();
        trace.end_tick = 900;
        trace
            .commands
            .retain(|command| command.tick < trace.end_tick);
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());

        run_trace(&mut simulation, &trace).unwrap();

        assert!(simulation
            .events()
            .iter()
            .any(|event| matches!(event.kind, SimulationEventKind::RelayActivated { index: 0 })));
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::UnitQueued {
                kind: UnitKind::Warden
            }
        )));
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::UnitDeployed {
                kind: UnitKind::Warden,
                ..
            }
        )));
        assert!(simulation
            .events()
            .iter()
            .any(|event| matches!(event.kind, SimulationEventKind::ResourcesCredited { .. })));
    }

    #[test]
    fn enemy_economy_funds_a_raid_and_damages_the_frontier() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        for relay in &mut simulation.relays {
            relay.active = true;
        }
        simulation.enemy_resources.primary = 90;
        simulation.enemy_raid_timer = 0.0;
        let health_before = simulation
            .structure(StructureKind::Relay(0))
            .expect("relay state")
            .health;

        simulation.fixed_step_with_dt(0.0);

        assert_eq!(simulation.enemy_raid_count, 1);
        assert!(simulation
            .events()
            .iter()
            .any(|event| matches!(event.kind, SimulationEventKind::EnemyRaidSpawned { .. })));
        assert!(
            simulation
                .structure(StructureKind::Relay(0))
                .expect("relay state")
                .health
                < health_before
        );
    }

    #[test]
    fn movement_without_selection_is_rejected() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let command = SemanticCommand::new(0, MOVE_SELECTED_ACTION)
            .with_payload(&[0.0_f32, 0.0_f32])
            .unwrap();

        assert!(simulation.apply_command(&command).is_err());
    }

    #[test]
    fn relay_restore_job_survives_selection_change() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let engineer = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Engineer).then_some(*id))
            .expect("mission includes an Engineer");
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let relay_position = simulation.relays[0].position;
        simulation.world.unit_mut(engineer).unwrap().position = relay_position;

        // The Engineer begins selected and starts its relay job.
        assert_eq!(simulation.world.select_ids(&[engineer], PLAYER, false), 1);
        simulation.fixed_step_with_dt(1.0);
        let progress_while_selected = simulation.relays[0].progress;
        assert!(progress_while_selected > 0.0);

        // Switching to another role must not cancel work already in progress.
        assert_eq!(simulation.world.select_ids(&[warden], PLAYER, false), 1);
        simulation.fixed_step_with_dt(1.0);
        assert!(
            simulation.relays[0].progress > progress_while_selected,
            "relay work should continue after the player selects a different role"
        );
        assert!(simulation.engineer_near(relay_position));
        assert!(!simulation.selected_engineer_near(relay_position));
    }

    #[test]
    fn engineer_repairs_the_most_damaged_nearby_lantern() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .unwrap();
        simulation.world.unit_mut(warden).unwrap().health = 80.0;

        simulation.fixed_step_with_dt(1.0 / 60.0);

        assert!(simulation.world.unit(warden).unwrap().health > 80.0);
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::UnitRepaired { target, .. } if target == warden.0
        )));
    }

    #[test]
    fn engineer_uses_one_repair_beam_and_falls_back_to_structures() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let engineer = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Engineer).then_some(*id))
            .expect("mission includes an Engineer");
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let engineer_position = simulation.world.unit(engineer).unwrap().position;
        simulation.world.unit_mut(warden).unwrap().position = engineer_position;
        simulation.world.unit_mut(warden).unwrap().health = 40.0;
        let relay_before = simulation
            .structure(StructureKind::Relay(0))
            .expect("relay state")
            .health;
        simulation
            .structures
            .get_mut(&StructureKind::Relay(0))
            .unwrap()
            .health = relay_before - 100.0;

        simulation.fixed_step_with_dt(1.0);

        assert_eq!(simulation.world.unit(warden).unwrap().health, 52.0);
        assert_eq!(
            simulation
                .structure(StructureKind::Relay(0))
                .expect("relay state")
                .health,
            relay_before - 100.0,
            "the Engineer cannot repair a unit and structure simultaneously"
        );

        let warden_max_health = simulation.world.unit(warden).unwrap().max_health;
        simulation.world.unit_mut(warden).unwrap().health = warden_max_health;
        let relay_position = simulation.relays[0].position;
        simulation.world.unit_mut(engineer).unwrap().position = relay_position;
        simulation.fixed_step_with_dt(1.0);
        assert_eq!(
            simulation
                .structure(StructureKind::Relay(0))
                .expect("relay state")
                .health,
            relay_before - 88.0
        );
        assert!(simulation
            .events()
            .iter()
            .any(|event| matches!(event.kind, SimulationEventKind::StructureRepaired { .. })));
    }

    #[test]
    fn resource_objective_stalls_under_contest_and_completes_with_support() {
        let mission = crate::missions::garden_below();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let (objective, target) = simulation
            .resource_objective_contract()
            .expect("garden should expose a resource objective");
        assert_eq!(objective.node_index, 1);
        assert_eq!(target, mission.salvage_nodes[1]);

        let surveyor = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .expect("garden Surveyor");
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("garden Warden");
        let needle = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Needle).then_some(*id))
            .expect("garden contesting Needle");

        simulation.world.unit_mut(surveyor).unwrap().position = target;
        simulation.world.unit_mut(needle).unwrap().position = target;
        simulation.fixed_step_with_dt(1.0);
        let state = simulation
            .resource_objective_state()
            .expect("objective state");
        assert!(state.contested);
        assert_eq!(state.progress_seconds, 0.0);
        assert_eq!(state.contested_seconds, 1.0);

        simulation.world.unit_mut(warden).unwrap().position = target;
        simulation.fixed_step_with_dt(3.0);
        let state = simulation
            .resource_objective_state()
            .expect("objective state");
        assert!(!state.contested);
        assert_eq!(state.progress_seconds, 3.0);

        simulation.world.unit_mut(needle).unwrap().position = Vec2::new(1_500.0, 900.0);
        simulation.fixed_step_with_dt(5.0);
        let state = simulation
            .resource_objective_state()
            .expect("objective state");
        assert!(state.completed);
        assert_eq!(state.fraction(objective), 1.0);

        // Completion is sticky even after the support line breaks or the
        // worker leaves, which makes the earned objective replay-safe.
        simulation.world.unit_mut(warden).unwrap().position = Vec2::ZERO;
        simulation.world.unit_mut(surveyor).unwrap().position = Vec2::ZERO;
        simulation.fixed_step_with_dt(1.0);
        assert!(
            simulation
                .resource_objective_state()
                .expect("objective state")
                .completed
        );
    }

    #[test]
    fn command_surge_reduces_incoming_damage_during_the_hold_window() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut baseline =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let mut surged = MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden_baseline = baseline
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("baseline Warden");
        let warden_surged = surged
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("surged Warden");
        let enemy_baseline = baseline.spawn(
            UnitKind::Needle,
            CHOIR,
            baseline.world.unit(warden_baseline).unwrap().position,
            95.0,
            130.0,
            SimulationModifiers::default(),
        );
        let enemy_surged = surged.spawn(
            UnitKind::Needle,
            CHOIR,
            surged.world.unit(warden_surged).unwrap().position,
            95.0,
            130.0,
            SimulationModifiers::default(),
        );
        baseline.world.unit_mut(enemy_baseline).unwrap().order = UnitOrder::Attack(warden_baseline);
        surged.world.unit_mut(enemy_surged).unwrap().order = UnitOrder::Attack(warden_surged);
        surged
            .activate_ability(warden_surged)
            .expect("Warden surge");

        baseline.fixed_step_with_dt(1.0);
        surged.fixed_step_with_dt(1.0);

        let baseline_health = baseline.world.unit(warden_baseline).unwrap().health;
        let surged_health = surged.world.unit(warden_surged).unwrap().health;
        assert!(
            surged_health > baseline_health,
            "surged Warden should absorb less damage: baseline={baseline_health}, surged={surged_health}"
        );
    }

    #[test]
    fn specialist_abilities_are_role_specific_and_cooldown_gated() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let engineer = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Engineer).then_some(*id))
            .expect("mission includes an Engineer");
        let surveyor = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .expect("mission includes a Surveyor");

        let surge = simulation.activate_ability(warden).expect("Warden surge");
        assert_eq!(surge, SpecialAbility::CommandSurge);
        assert!(simulation.command_surge_remaining(warden) > 0.0);
        assert_eq!(
            simulation.activate_ability(warden),
            Err(AbilityError::Cooldown)
        );

        let warden_position = simulation.world.unit(warden).unwrap().position;
        simulation.world.unit_mut(engineer).unwrap().position = warden_position;
        simulation.world.unit_mut(warden).unwrap().health = 40.0;
        let repair = simulation
            .activate_ability(engineer)
            .expect("Engineer emergency repair");
        assert_eq!(repair, SpecialAbility::EmergencyRepair);
        assert!(simulation.world.unit(warden).unwrap().health > 40.0);

        let scan = simulation
            .activate_ability(surveyor)
            .expect("Surveyor scan pulse");
        assert_eq!(scan, SpecialAbility::ScanPulse);
        assert!(simulation.scan_pulse.is_some());
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::AbilityActivated {
                ability: SpecialAbility::ScanPulse,
                ..
            }
        )));
    }

    #[test]
    fn supply_module_is_a_powered_timed_structure_build() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        assert_eq!(simulation.supply.capacity(), 12);
        simulation
            .queue_supply_module()
            .expect("module should queue");
        assert_eq!(
            simulation.supply.capacity(),
            12,
            "capacity changes on completion"
        );
        assert_eq!(simulation.supply_module_percent(), Some(0));
        assert_eq!(
            simulation.queue_supply_module(),
            Err(StructureCommandError::Busy)
        );

        simulation.fixed_step_with_dt(6.0);
        assert_eq!(simulation.supply.capacity(), 16);
        assert_eq!(simulation.supply_module_level, 1);
        assert!(simulation.supply_module_progress.is_none());
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::StructureBuildCompleted { .. }
        )));
    }

    #[test]
    fn cancelling_a_surveyor_refunds_salvage_flux_and_reserved_supply() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let salvage_before = simulation.resources.amount();
        let flux_before = simulation.flux;
        let supply_before = simulation.supply.used();

        simulation
            .queue_unit(UnitKind::Surveyor)
            .expect("initial economy can queue one Surveyor");
        assert_eq!(simulation.resources.amount(), salvage_before - 60);
        assert_eq!(simulation.flux, flux_before - 1);
        assert_eq!(simulation.supply.used(), supply_before + 1);

        let receipt = simulation
            .cancel_queued_unit(0)
            .expect("powered Fabricator can cancel a queued unit");
        assert_eq!(receipt.kind, UnitKind::Surveyor);
        assert_eq!(receipt.refunded_salvage, 45);
        assert_eq!(receipt.refunded_flux, 1);
        assert_eq!(receipt.released_supply, 1);
        assert_eq!(simulation.resources.amount(), salvage_before - 15);
        assert_eq!(simulation.flux, flux_before);
        assert_eq!(simulation.supply.used(), supply_before);
        assert!(simulation.production.items().is_empty());
    }

    #[test]
    fn cancelling_a_later_slot_keeps_the_active_front_job_busy() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        simulation.resources.credit(200);
        simulation
            .queue_unit(UnitKind::Warden)
            .expect("Warden should fit the first slot");
        simulation
            .queue_unit(UnitKind::Engineer)
            .expect("Engineer should fit the second slot");
        let front_before = simulation.production.items().front().copied().unwrap();
        let supply_before = simulation.supply.used();

        let receipt = simulation
            .cancel_queued_unit(1)
            .expect("a busy Fabricator can cancel a later slot");
        assert_eq!(receipt.kind, UnitKind::Engineer);
        assert_eq!(receipt.refunded_salvage, 52);
        assert_eq!(receipt.released_supply, 1);
        assert_eq!(simulation.production.items().len(), 1);
        assert_eq!(
            simulation.production.items().front().copied(),
            Some(front_before)
        );
        assert_eq!(simulation.supply.used(), supply_before - 1);

        simulation.fixed_step_with_dt(1.0);
        let front_after = simulation.production.items().front().copied().unwrap();
        assert!(front_after.remaining_seconds < front_before.remaining_seconds);
    }

    #[test]
    fn cancelling_an_invalid_slot_is_side_effect_free() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let salvage_before = simulation.resources.amount();
        let flux_before = simulation.flux;
        let supply_before = simulation.supply.used();

        assert_eq!(
            simulation.cancel_queued_unit(0),
            Err(ProductionCancelCommandError::InvalidIndex)
        );
        assert_eq!(simulation.resources.amount(), salvage_before);
        assert_eq!(simulation.flux, flux_before);
        assert_eq!(simulation.supply.used(), supply_before);
    }

    #[test]
    fn cancelling_while_fabricator_is_offline_preserves_the_queue_and_wallet() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        simulation
            .queue_unit(UnitKind::Warden)
            .expect("Warden should queue before taking the Fabricator offline");
        let item_before = simulation.production.items().front().copied();
        let salvage_before = simulation.resources.amount();
        let flux_before = simulation.flux;
        let supply_before = simulation.supply.used();
        simulation.power.set_online(FABRICATOR_NODE, false);

        assert_eq!(
            simulation.cancel_queued_unit(0),
            Err(ProductionCancelCommandError::FabricatorOffline)
        );
        assert_eq!(simulation.production.items().front().copied(), item_before);
        assert_eq!(simulation.resources.amount(), salvage_before);
        assert_eq!(simulation.flux, flux_before);
        assert_eq!(simulation.supply.used(), supply_before);
    }

    #[test]
    fn raid_forecast_emits_one_readable_warning_before_spawn() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());

        assert_eq!(simulation.raid_state().number, 1);
        assert_eq!(simulation.raid_state().phase, RaidPhase::Teaching);

        // Fund the first contact manually so this test isolates the warning
        // contract rather than depending on economy accumulation.
        simulation.enemy_resources.primary = 90;
        simulation.fixed_step_with_dt(FIRST_ENEMY_RAID_DELAY - RAID_WARNING_WINDOW);
        let warning = simulation.raid_state();
        assert_eq!(warning.phase, RaidPhase::Warning);
        assert_eq!(warning.kind, UnitKind::Needle);
        assert!(warning.seconds_remaining <= RAID_WARNING_WINDOW + f32::EPSILON);
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::EnemyRaidTelegraph {
                number: 1,
                kind: UnitKind::Needle,
                ..
            }
        )));

        simulation.fixed_step_with_dt(RAID_WARNING_WINDOW);
        assert_eq!(simulation.enemy_raid_count, 1);
        assert_eq!(
            simulation
                .events()
                .iter()
                .filter(|event| matches!(
                    event.kind,
                    SimulationEventKind::EnemyRaidTelegraph { .. }
                ))
                .count(),
            1,
            "a raid should not repeat its warning every fixed tick"
        );
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::EnemyRaidSpawned {
                kind: UnitKind::Needle,
                ..
            }
        )));
    }

    #[test]
    fn later_raids_escalate_pressure_without_changing_the_first_contact() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        simulation.relays[0].active = true;
        simulation.enemy_resources.primary = 90;
        simulation.enemy_raid_timer = 0.0;
        simulation.fixed_step_with_dt(0.0);
        let raid_ids: Vec<UnitId> = simulation
            .events()
            .iter()
            .filter_map(|event| match event.kind {
                SimulationEventKind::EnemyRaidSpawned { unit_id, .. } => Some(UnitId(unit_id)),
                _ => None,
            })
            .collect();
        assert_eq!(raid_ids.len(), 1);
        let first_health = simulation.world.unit(raid_ids[0]).unwrap().max_health;
        assert_eq!(
            first_health, 82.0,
            "the teaching contact keeps its authored size"
        );
        let relay_after_first = simulation
            .structure(StructureKind::Relay(0))
            .expect("relay state")
            .health;

        simulation.enemy_resources.primary = 90;
        simulation.enemy_raid_timer = 0.0;
        simulation.fixed_step_with_dt(0.0);
        let raid_ids: Vec<UnitId> = simulation
            .events()
            .iter()
            .filter_map(|event| match event.kind {
                SimulationEventKind::EnemyRaidSpawned { unit_id, .. } => Some(UnitId(unit_id)),
                _ => None,
            })
            .collect();
        assert_eq!(raid_ids.len(), 2);
        let second_health = simulation.world.unit(raid_ids[1]).unwrap().max_health;
        assert!(
            second_health > first_health,
            "later contacts should earn more durability: first={first_health}, second={second_health}"
        );
        assert_eq!(
            simulation
                .structure(StructureKind::Relay(0))
                .expect("relay state")
                .health,
            relay_after_first - (STANDARD_RAID_RELAY_DAMAGE + LATE_RAID_RELAY_DAMAGE_STEP),
            "relay pressure climbs one step after the teaching contact"
        );
    }

    #[test]
    fn combat_target_and_telegraph_state_is_persistent_and_queryable() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let needle = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Needle).then_some(*id))
            .expect("mission includes a Needle");
        let origin = simulation.world.unit(warden).unwrap().position;
        simulation.world.unit_mut(needle).unwrap().position = origin + Vec2::X * 40.0;
        simulation.world.unit_mut(warden).unwrap().order = UnitOrder::Attack(needle);

        simulation.fixed_step_with_dt(0.5);

        let contact = simulation.combat_contact(warden).expect("combat contact");
        assert_eq!(contact.target, Some(needle));
        assert_eq!(contact.state, CombatContactState::Attacking);
        assert!(contact.health_fraction > 0.0);
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::TargetAcquired { attacker, target }
                if attacker == warden.0 && target == needle.0
        )));
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::AttackTelegraph { attacker, target, .. }
                if attacker == warden.0 && target == needle.0
        )));
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::AttackLanded { attacker, target }
                if attacker == warden.0 && target == needle.0
        )));

        simulation.world.unit_mut(warden).unwrap().order = UnitOrder::Idle;
        simulation.fixed_step_with_dt(0.0);
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::TargetLost { attacker } if attacker == warden.0
        )));
    }

    #[test]
    fn explicit_attack_stops_at_weapon_range_and_still_resolves_damage() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let origin = simulation.world.unit(warden).unwrap().position;
        let needle = simulation.spawn(
            UnitKind::Needle,
            CHOIR,
            origin + Vec2::X * 500.0,
            150.0,
            0.0,
            SimulationModifiers::default(),
        );
        simulation.world.unit_mut(warden).unwrap().order = UnitOrder::Attack(needle);

        // Three seconds is long enough for the Warden's authored 175 units/s
        // speed to reach the firing line from the 500-unit setup distance.
        simulation.fixed_step_with_dt(3.0);

        let warden_unit = simulation.world.unit(warden).unwrap();
        let needle_unit = simulation.world.unit(needle).unwrap();
        let distance = warden_unit.position.distance(needle_unit.position);
        let weapon_range = UnitKind::Warden.combat().range;
        assert!(
            distance < weapon_range,
            "the Warden should be inside its firing range: distance={distance}, range={weapon_range}"
        );
        assert!(
            distance > weapon_range * 0.85,
            "the Warden should hold a readable firing line: distance={distance}"
        );
        assert_eq!(warden_unit.order, UnitOrder::Attack(needle));
        assert!(
            needle_unit.health < needle_unit.max_health,
            "the attack must resolve after the movement clamp"
        );
    }

    #[test]
    fn ordinary_weapon_pulses_use_the_engine_combat_profile_bridge() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let origin = Vec2::new(-1_500.0, 850.0);
        simulation.world.unit_mut(warden).unwrap().position = origin;
        simulation.world.unit_mut(warden).unwrap().speed = 0.0;
        let needle = simulation.spawn(
            UnitKind::Needle,
            CHOIR,
            origin + Vec2::X * 100.0,
            200.0,
            0.0,
            SimulationModifiers::default(),
        );
        simulation.world.unit_mut(warden).unwrap().order = UnitOrder::Attack(needle);

        let profile = simulation.world.unit(warden).unwrap().combat;
        assert_eq!(profile.range, UnitKind::Warden.combat().range);
        assert!(
            profile.damage > 0.0,
            "spawned units expose engine combat data"
        );
        let before = simulation.world.unit(needle).unwrap().health;

        simulation.fixed_step_with_dt(0.5);

        let after = simulation.world.unit(needle).unwrap().health;
        // Warden DPS (32) * 0.5s pulse, reduced by the Needle's normalized
        // engine armor bridge (1 authored armor * 0.03).
        assert!((before - after - 15.52).abs() < 0.001);
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::AttackLanded { attacker, target }
                if attacker == warden.0 && target == needle.0
        )));
    }

    #[test]
    fn authored_weapon_period_prevents_fixed_step_hit_spam() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let origin = Vec2::new(-1_500.0, 850.0);
        simulation.world.unit_mut(warden).unwrap().position = origin;
        simulation.world.unit_mut(warden).unwrap().speed = 0.0;
        let needle = simulation.spawn(
            UnitKind::Needle,
            CHOIR,
            origin + Vec2::X * 100.0,
            200.0,
            0.0,
            SimulationModifiers::default(),
        );
        simulation.world.unit_mut(warden).unwrap().order = UnitOrder::Attack(needle);

        let landed = |simulation: &MissionSimulation| {
            simulation
                .events()
                .iter()
                .filter(|event| {
                    matches!(
                        event.kind,
                        SimulationEventKind::AttackLanded { attacker, target }
                            if attacker == warden.0 && target == needle.0
                    )
                })
                .count()
        };

        simulation.fixed_step_with_dt(0.01);
        assert_eq!(
            landed(&simulation),
            1,
            "the opening pulse fires immediately"
        );
        simulation.fixed_step_with_dt(0.20);
        simulation.fixed_step_with_dt(0.28);
        assert_eq!(
            landed(&simulation),
            1,
            "cooldown suppresses fixed-step spam"
        );
        simulation.fixed_step_with_dt(0.03);
        assert_eq!(
            landed(&simulation),
            2,
            "the next authored pulse fires on cadence"
        );
    }

    #[test]
    fn hold_position_guards_in_weapon_range_without_chasing() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let origin = simulation.world.unit(warden).unwrap().position;
        let needle = simulation.spawn(
            UnitKind::Needle,
            CHOIR,
            origin + Vec2::X * 110.0,
            150.0,
            0.0,
            SimulationModifiers::default(),
        );
        simulation.world.unit_mut(warden).unwrap().order = UnitOrder::Hold;
        let origin_before = simulation.world.unit(warden).unwrap().position;
        let health_before = simulation.world.unit(needle).unwrap().health;

        simulation.fixed_step_with_dt(1.0);

        let warden_unit = simulation.world.unit(warden).unwrap();
        assert_eq!(warden_unit.order, UnitOrder::Hold);
        assert_eq!(warden_unit.position, origin_before);
        assert!(simulation.world.unit(needle).unwrap().health < health_before);
        assert_eq!(simulation.combat_target(warden), Some(needle));
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::TargetAcquired { attacker, target }
                if attacker == warden.0 && target == needle.0
        )));
    }

    #[test]
    fn hold_position_does_not_acquire_or_pursue_outside_weapon_range() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let origin = simulation.world.unit(warden).unwrap().position;
        let needle = simulation.spawn(
            UnitKind::Needle,
            CHOIR,
            origin + Vec2::X * 320.0,
            150.0,
            0.0,
            SimulationModifiers::default(),
        );
        simulation.world.unit_mut(warden).unwrap().order = UnitOrder::Hold;

        simulation.fixed_step_with_dt(1.0);

        let warden_unit = simulation.world.unit(warden).unwrap();
        assert_eq!(warden_unit.order, UnitOrder::Hold);
        assert_eq!(warden_unit.position, origin);
        assert_eq!(simulation.combat_target(warden), None);
        assert_eq!(simulation.world.unit(needle).unwrap().health, 150.0);
        assert!(!simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::TargetAcquired { attacker, target }
                if attacker == warden.0 && target == needle.0
        )));
    }

    #[test]
    fn choir_retreat_marker_fires_on_low_health_move_and_recovery() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let id = simulation.spawn(
            UnitKind::Needle,
            CHOIR,
            Vec2::new(300.0, 300.0),
            100.0,
            120.0,
            SimulationModifiers::default(),
        );
        simulation.world.unit_mut(id).unwrap().health = 30.0;
        simulation.world.unit_mut(id).unwrap().order = UnitOrder::Move(Vec2::ZERO);
        simulation.fixed_step_with_dt(0.0);

        assert_eq!(
            simulation.combat_contact(id).unwrap().state,
            CombatContactState::Retreating
        );
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::UnitRetreating {
                unit_id,
                kind: UnitKind::Needle
            } if unit_id == id.0
        )));

        simulation.world.unit_mut(id).unwrap().health = 100.0;
        simulation.world.unit_mut(id).unwrap().order = UnitOrder::Idle;
        simulation.fixed_step_with_dt(0.0);
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::UnitRecovered {
                unit_id,
                kind: UnitKind::Needle
            } if unit_id == id.0
        )));
    }

    #[test]
    fn attack_move_routes_around_obstacles_and_keeps_attack_stance() {
        let mission = crate::missions::voice_in_conduit_twelve();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let unit_id = simulation.world.units()[0].id;
        let start = simulation.world.units()[0].position;
        simulation.world.select_point(start, PLAYER, false);
        assert!(simulation.world.selection().contains(unit_id));

        let destination = Vec2::new(start.x, -200.0);
        assert!(
            simulation.nav.segment_blocked(start, destination),
            "test destination should cross a corridor wall"
        );

        simulation.issue_attack_move_order(destination, false);

        let UnitOrder::AttackMove(first_waypoint) = simulation.world.unit(unit_id).unwrap().order
        else {
            panic!("expected a routed AttackMove order at the first waypoint");
        };
        assert_ne!(first_waypoint, destination);
        assert!(simulation.player_paths.contains_key(&unit_id));

        // Arrival at the first waypoint must preserve attack-move semantics;
        // otherwise combat target acquisition is lost while following the
        // remainder of the route.
        {
            let unit = simulation.world.unit_mut(unit_id).unwrap();
            unit.position = first_waypoint;
            unit.order = UnitOrder::Idle;
        }
        simulation.advance_player_paths();
        assert!(matches!(
            simulation.world.unit(unit_id).unwrap().order,
            UnitOrder::AttackMove(_)
        ));
    }

    #[test]
    fn terms_ridge_warden_can_close_and_fire_on_first_needle() {
        let mission = crate::missions::terms_of_salvage();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let objective = mission
            .terrain_control_objective
            .expect("Terms authors a ridge control objective");
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("Terms includes a Warden");
        let needle = simulation
            .world
            .units()
            .iter()
            .find(|unit| simulation.kinds.get(&unit.id) == Some(&UnitKind::Needle))
            .map(|unit| unit.id)
            .expect("Terms includes a Needle");
        simulation.world.unit_mut(warden).unwrap().position = objective.target;
        simulation.world.select_ids(&[warden], PLAYER, false);
        assert!(simulation.issue_attack_kind(UnitKind::Needle));
        let health_before = simulation.world.unit(needle).unwrap().health;

        for _ in 0..12 * 60 {
            simulation.fixed_step_with_dt(1.0 / 60.0);
        }

        let attacker = simulation.world.unit(warden).unwrap();
        let target = simulation.world.unit(needle).unwrap();
        assert!(
            target.health < health_before,
            "Warden never fired: separation {:.2}, authored reach {:.2}, order {:?}",
            attacker.position.distance(target.position),
            UnitKind::Warden.combat().range + target.radius * 0.5,
            attacker.order,
        );
    }

    #[test]
    fn bell_mine_detonates_once_and_hits_a_clumped_squad() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let warden = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("mission includes a Warden");
        let engineer = simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Engineer).then_some(*id))
            .expect("mission includes an Engineer");
        let origin = Vec2::new(-700.0, 0.0);
        simulation.world.unit_mut(warden).unwrap().position = origin + Vec2::X * 40.0;
        simulation.world.unit_mut(engineer).unwrap().position = origin + Vec2::X * 90.0;
        let mine = simulation.spawn(
            UnitKind::BellMine,
            CHOIR,
            origin,
            110.0,
            80.0,
            SimulationModifiers::default(),
        );
        // The mine starts idle on purpose. Proximity acquisition is the trap's
        // authored gameplay contract; it should not depend on enemy AI having
        // already assigned an explicit Attack order.
        let warden_before = simulation.world.unit(warden).unwrap().health;
        let engineer_before = simulation.world.unit(engineer).unwrap().health;

        simulation.fixed_step_with_dt(1.0 / 60.0);

        assert_eq!(simulation.world.unit(mine).unwrap().health, 0.0);
        assert!(simulation.world.unit(warden).unwrap().health < warden_before);
        assert!(simulation.world.unit(engineer).unwrap().health < engineer_before);
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::UnitDetonated {
                unit_id,
                kind: UnitKind::BellMine
            } if unit_id == mine.0
        )));
        assert!(simulation.events().iter().any(|event| matches!(
            event.kind,
            SimulationEventKind::UnitDestroyed {
                unit_id,
                kind: UnitKind::BellMine
            } if unit_id == mine.0
        )));
        let landed = simulation
            .events()
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    SimulationEventKind::AttackLanded { attacker, .. } if attacker == mine.0
                )
            })
            .count();
        assert_eq!(landed, 2, "one blast should hit both clustered Lanterns");

        let warden_after = simulation.world.unit(warden).unwrap().health;
        let engineer_after = simulation.world.unit(engineer).unwrap().health;
        simulation.fixed_step_with_dt(1.0 / 60.0);
        assert!(
            simulation.world.unit(warden).unwrap().health >= warden_after,
            "a spent mine cannot damage again; any increase is Engineer repair"
        );
        assert_eq!(
            simulation.world.unit(engineer).unwrap().health,
            engineer_after
        );
    }
}
