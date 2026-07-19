//! Renderer-free tactical state for Last Light missions.

use std::collections::{HashMap, VecDeque};

use aurora_engine::{
    mark_obstacles, Aabb, DeterministicSimulation, FactionId, NavGrid, PowerGrid, PowerNode,
    PowerNodeId, ProductionQueue, QueueError, ResourceBank, ResourceSet, RtsWorld, SemanticCommand,
    StableStateHasher, StateHash, SupplyLedger, TechGraph, TechId, TerrainZone, UnitId, UnitOrder,
};
use glam::Vec2;
use serde::Serialize;

use crate::mission_state::{Relay, StructureKind, StructureState};
use crate::missions::{MissionDef, VictoryCondition};
use crate::units::{UnitKind, CHOIR, PLAYER};

pub const MAP_SIZE: Vec2 = Vec2::new(3600.0, 2200.0);
pub const NAV_CELL_SIZE: f32 = 40.0;
const DEFAULT_FIXED_TICK_HZ: u32 = 60;
const EVENT_LOG_CAPACITY: usize = 256;
const COMBAT_BUFFER_CAPACITY: usize = 32;
const PATH_ADVANCE_BUFFER_CAPACITY: usize = 32;
pub const FABRICATOR_NODE: PowerNodeId = PowerNodeId(0);
pub const TECH_RELAY_NETWORK: TechId = TechId(1);
pub const TECH_LUMEN_CORE: TechId = TechId(2);

pub const SELECT_ALL_ACTION: &str = "last_light.select_all";
pub const SELECT_KIND_ACTION: &str = "last_light.select_kind";
pub const MOVE_SELECTED_ACTION: &str = "last_light.move_selected";
pub const ATTACK_MOVE_SELECTED_ACTION: &str = "last_light.attack_move_selected";
pub const PATROL_SELECTED_ACTION: &str = "last_light.patrol_selected";
pub const QUEUE_UNIT_ACTION: &str = "last_light.queue_unit";
pub const ATTACK_KIND_ACTION: &str = "last_light.attack_kind";

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

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SimulationEventKind {
    CommandAccepted { action: String },
    RelayActivated { index: usize },
    ResourcesCredited { amount: u32 },
    ResourcesDelivered { salvage: u32, flux: u32 },
    UnitQueued { kind: UnitKind },
    UnitDeployed { unit_id: u32, kind: UnitKind },
    UnitSpawned { unit_id: u32, kind: UnitKind },
    AttackLanded { attacker: u32, target: u32 },
    DamageApplied { target: u32 },
    UnitRepaired { engineer: u32, target: u32 },
    StructureRepaired { structure: String },
    UnitDestroyed { unit_id: u32, kind: UnitKind },
    BossReinforced,
    EnemyRaidSpawned { unit_id: u32, kind: UnitKind },
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

/// The first extracted Last Light simulation seam. It owns the live tactical
/// roster, unit identity, navigation grid, and path following used by both the
/// interactive game and headless semantic traces.
pub struct MissionSimulation {
    pub world: RtsWorld,
    pub kinds: HashMap<UnitId, UnitKind>,
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
    pub reactor_position: Option<Vec2>,
    pub enemy_resources: ResourceSet,
    pub enemy_raid_count: u32,
    pub salvage_delivered: u32,
    pub production: ProductionQueue,
    pub power: PowerGrid,
    pub fabricator_position: Vec2,
    pub rally_point: Option<Vec2>,
    pub outcome: MissionOutcome,
    tick: u64,
    fixed_dt: f32,
    resource_tick: f32,
    modifiers: SimulationModifiers,
    victory_condition: VictoryCondition,
    boss_reinforced: bool,
    events: VecDeque<SimulationEvent>,
    pending_events: VecDeque<SimulationEvent>,
    path_advance_ids: Vec<UnitId>,
    combat_snapshot: Vec<(UnitId, Vec2, bool)>,
    combat_attacks: Vec<(UnitId, UnitId, f32)>,
    support_repairs: Vec<(UnitId, UnitId, f32)>,
    structure_repairs: Vec<(UnitId, StructureKind, f32)>,
    enemy_income_tick: f32,
    enemy_raid_timer: f32,
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
        mark_obstacles(&mut nav, &mission.obstacles);
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
            reactor_position: mission.reactor_position,
            enemy_resources: ResourceSet::new(0, 0),
            enemy_raid_count: 0,
            salvage_delivered: 0,
            production: ProductionQueue::new(5),
            power,
            fabricator_position: mission.fabricator_position,
            rally_point: None,
            outcome: MissionOutcome::InProgress,
            tick: 0,
            fixed_dt: 1.0 / DEFAULT_FIXED_TICK_HZ as f32,
            resource_tick: 0.0,
            modifiers,
            victory_condition: mission.victory,
            boss_reinforced: false,
            events: VecDeque::with_capacity(EVENT_LOG_CAPACITY),
            pending_events: VecDeque::with_capacity(EVENT_LOG_CAPACITY),
            path_advance_ids: Vec::with_capacity(PATH_ADVANCE_BUFFER_CAPACITY),
            combat_snapshot: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            combat_attacks: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            support_repairs: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            structure_repairs: Vec::with_capacity(COMBAT_BUFFER_CAPACITY),
            enemy_income_tick: 0.0,
            enemy_raid_timer: 42.0,
            destroyed_by_kind: HashMap::new(),
        };
        for spawn in &mission.player_spawns {
            let id = simulation.spawn(
                spawn.kind,
                PLAYER,
                spawn.position,
                spawn.health,
                spawn.speed,
                simulation.modifiers,
            );
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
        }
        if faction == PLAYER {
            // Starting units and deployed production reserve the same ledger;
            // a failed queue is rejected before a unit can exceed supply.
            let _ = self.supply.try_add(kind.supply_cost());
        }
        self.kinds.insert(id, kind);
        id
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
            let UnitOrder::Move(destination) = unit.order else {
                continue;
            };
            if self.nav.segment_blocked(unit.position, destination) {
                let mut path: VecDeque<Vec2> =
                    self.nav.find_path(unit.position, destination).into();
                if let Some(first) = path.pop_front() {
                    if let Some(unit) = self.world.unit_mut(id) {
                        unit.order = UnitOrder::Move(first);
                    }
                    self.player_paths.insert(id, path);
                    continue;
                }
            }
            self.player_paths.remove(&id);
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
                continue;
            };
            if !matches!(unit.order, UnitOrder::Idle) {
                continue;
            }
            let done = match self.player_paths.get_mut(&id) {
                Some(queue) => {
                    if let Some(next) = queue.pop_front() {
                        if let Some(unit) = self.world.unit_mut(id) {
                            unit.order = UnitOrder::Move(next);
                        }
                    }
                    queue.is_empty()
                }
                None => true,
            };
            if done {
                self.player_paths.remove(&id);
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

    pub fn destroyed_count(&self, kind: UnitKind) -> u32 {
        self.destroyed_by_kind.get(&kind).copied().unwrap_or(0)
    }

    pub fn structure(&self, kind: StructureKind) -> Option<StructureState> {
        self.structures.get(&kind).copied()
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
            .enqueue(recipe, &mut self.resources)
            .map_err(ProductionCommandError::Queue)?;
        self.flux -= secondary_cost;
        let _ = self.supply.try_add(supply_cost);
        self.record(SimulationEventKind::UnitQueued { kind });
        Ok(())
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
            if self.selected_engineer_near(position) {
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
                let (health, speed) = match kind {
                    UnitKind::Warden => (155.0, 175.0),
                    UnitKind::Engineer => (115.0, 150.0),
                    UnitKind::Surveyor => (90.0, 215.0),
                    UnitKind::Needle | UnitKind::Canticle | UnitKind::BellMine => continue,
                };
                // Queue reservation becomes a live unit reservation at deployment.
                self.supply.release(kind.supply_cost());
                let id = self.spawn(
                    kind,
                    PLAYER,
                    self.rally_point.unwrap_or(self.fabricator_position) + offset,
                    health,
                    speed,
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
        if self.enemy_raid_timer > 0.0 || self.enemy_resources.primary < 90 {
            return;
        }
        let living_enemy_count = self
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == CHOIR && unit.alive())
            .count();
        if living_enemy_count >= 18 {
            self.enemy_raid_timer = 12.0;
            return;
        }
        let anchor = self
            .relays
            .iter()
            .find(|relay| relay.active)
            .map(|relay| relay.position)
            .unwrap_or(self.fabricator_position);
        let kind = if self.enemy_raid_count % 3 == 2 {
            UnitKind::BellMine
        } else {
            UnitKind::Needle
        };
        let offset = if self.enemy_raid_count.is_multiple_of(2) {
            Vec2::new(280.0, -190.0)
        } else {
            Vec2::new(-250.0, 170.0)
        };
        let (health, speed) = match kind {
            UnitKind::Needle => (95.0, 130.0),
            UnitKind::BellMine => (110.0, 80.0),
            _ => unreachable!("raid roster is enemy-only"),
        };
        let id = self.spawn(kind, CHOIR, anchor + offset, health, speed, self.modifiers);
        if let Some((index, _)) = self
            .relays
            .iter()
            .enumerate()
            .find(|(_, relay)| relay.active)
        {
            let _ = self.damage_structure(StructureKind::Relay(index), 18.0);
        }
        self.enemy_resources.primary -= 90;
        self.enemy_raid_count += 1;
        self.enemy_raid_timer = 31.0;
        self.record(SimulationEventKind::EnemyRaidSpawned {
            unit_id: id.0,
            kind,
        });
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
        for unit in self.world.units() {
            let target = match unit.order {
                UnitOrder::Attack(target) => Some(target),
                UnitOrder::AttackMove(_) => self
                    .world
                    .units()
                    .iter()
                    .filter(|candidate| {
                        candidate.faction == CHOIR
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
                _ => None,
            };
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
            let target_kind = self.kinds.get(&target).copied().unwrap_or(UnitKind::Warden);
            let target_profile = target_kind.combat();
            let target_terrain_scale = self
                .terrain_zones
                .iter()
                .find(|zone| zone.contains(target_position))
                .map(|zone| zone.damage_multiplier(attacker_elevation))
                .unwrap_or(1.0);
            if unit.position.distance(target_position) < profile.range {
                self.combat_attacks.push((
                    unit.id,
                    target,
                    (profile.damage_per_second
                        * profile.damage_type.multiplier(target_profile.armor_class)
                        * target_terrain_scale
                        - target_profile.armor * 0.45)
                        .max(0.0)
                        * dt.max(0.0)
                        * if unit.faction == PLAYER {
                            self.modifiers.player_damage_scale
                        } else {
                            1.0
                        },
                ));
            }
        }

        for index in 0..self.combat_attacks.len() {
            let (attacker, target, amount) = self.combat_attacks[index];
            let Some(target_faction) = self.world.unit(target).map(|unit| unit.faction) else {
                continue;
            };
            let amount = if target_faction == PLAYER {
                amount * self.modifiers.player_damage_taken_scale
            } else {
                amount
            };
            let destroyed = if let Some(unit) = self.world.unit_mut(target) {
                let was_alive = unit.alive();
                unit.health = (unit.health - amount).max(0.0);
                was_alive && !unit.alive()
            } else {
                false
            };
            self.record(SimulationEventKind::AttackLanded {
                attacker: attacker.0,
                target: target.0,
            });
            if destroyed {
                let kind = self.kinds[&target];
                self.release_supply_and_record(target);
                *self.destroyed_by_kind.entry(kind).or_insert(0) += 1;
                self.record(SimulationEventKind::UnitDestroyed {
                    unit_id: target.0,
                    kind,
                });
            }
        }
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
            if let Some(target) =
                self.world
                    .most_damaged_ally_in_range(engineer.id, PLAYER, REPAIR_RANGE)
            {
                self.support_repairs
                    .push((engineer.id, target, REPAIR_PER_SECOND * dt.max(0.0)));
            }
            if let Some((kind, _)) = self
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
        self.world.update(dt);
        self.advance_player_paths();
        self.update_relays(dt);
        self.update_economy(dt);
        self.update_engineer_repairs(dt);
        self.update_combat(dt);
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
        hasher.write_u64(self.production.items().len() as u64);
        for item in self.production.items() {
            hasher.write_u64(u64::from(item.product.0));
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
}
