//! Renderer-free tactical state for Last Light missions.

use std::collections::{HashMap, VecDeque};

use aurora_engine::{
    mark_obstacles, Aabb, DeterministicSimulation, FactionId, NavGrid, PowerGrid, PowerNode,
    PowerNodeId, ProductionQueue, QueueError, ResourceBank, RtsWorld, SemanticCommand,
    StableStateHasher, StateHash, UnitId, UnitOrder,
};
use glam::Vec2;
use serde::Serialize;

use crate::mission_state::Relay;
use crate::missions::{MissionDef, VictoryCondition};
use crate::units::{UnitKind, CHOIR, PLAYER};

pub const MAP_SIZE: Vec2 = Vec2::new(2600.0, 1460.0);
pub const NAV_CELL_SIZE: f32 = 40.0;
const DEFAULT_FIXED_TICK_HZ: u32 = 60;
const EVENT_LOG_CAPACITY: usize = 256;
pub const FABRICATOR_NODE: PowerNodeId = PowerNodeId(0);

pub const SELECT_ALL_ACTION: &str = "last_light.select_all";
pub const SELECT_KIND_ACTION: &str = "last_light.select_kind";
pub const MOVE_SELECTED_ACTION: &str = "last_light.move_selected";
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
    UnitQueued { kind: UnitKind },
    UnitDeployed { unit_id: u32, kind: UnitKind },
    UnitSpawned { unit_id: u32, kind: UnitKind },
    AttackLanded { attacker: u32, target: u32 },
    DamageApplied { target: u32 },
    UnitDestroyed { unit_id: u32, kind: UnitKind },
    BossReinforced,
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
    FabricatorOffline,
    UnsupportedUnit,
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
    pub production: ProductionQueue,
    pub power: PowerGrid,
    pub fabricator_position: Vec2,
    pub outcome: MissionOutcome,
    tick: u64,
    fixed_dt: f32,
    resource_tick: f32,
    modifiers: SimulationModifiers,
    victory_condition: VictoryCondition,
    boss_reinforced: bool,
    events: VecDeque<SimulationEvent>,
    pending_events: VecDeque<SimulationEvent>,
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
            production: ProductionQueue::new(5),
            power,
            fabricator_position: mission.fabricator_position,
            outcome: MissionOutcome::InProgress,
            tick: 0,
            fixed_dt: 1.0 / DEFAULT_FIXED_TICK_HZ as f32,
            resource_tick: 0.0,
            modifiers,
            victory_condition: mission.victory,
            boss_reinforced: false,
            events: VecDeque::with_capacity(EVENT_LOG_CAPACITY),
            pending_events: VecDeque::new(),
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
            self.record(SimulationEventKind::UnitDestroyed {
                unit_id: target.0,
                kind: self.kinds[&target],
            });
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
        let ids: Vec<UnitId> = self.player_paths.keys().copied().collect();
        for id in ids {
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
            }
        }
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
        if !self.power.is_powered(FABRICATOR_NODE) {
            return Err(ProductionCommandError::FabricatorOffline);
        }
        let Some(mut recipe) = kind.recipe() else {
            return Err(ProductionCommandError::UnsupportedUnit);
        };
        recipe.build_millis =
            (recipe.build_millis as f32 * self.modifiers.production_time_scale) as u32;
        self.production
            .enqueue(recipe, &mut self.resources)
            .map_err(ProductionCommandError::Queue)?;
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

        if !self.power.is_powered(FABRICATOR_NODE) {
            return;
        }
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
            let id = self.spawn(
                kind,
                PLAYER,
                self.fabricator_position + offset,
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

    fn update_combat(&mut self, dt: f32) {
        let snapshot: HashMap<UnitId, (Vec2, bool)> = self
            .world
            .units()
            .iter()
            .map(|unit| (unit.id, (unit.position, unit.alive())))
            .collect();
        let attacks: Vec<(UnitId, UnitId, f32)> = self
            .world
            .units()
            .iter()
            .filter_map(|unit| {
                let UnitOrder::Attack(target) = unit.order else {
                    return None;
                };
                let (target_position, true) = snapshot.get(&target).copied()? else {
                    return None;
                };
                let profile = self.kinds.get(&unit.id)?.combat();
                (unit.position.distance(target_position) < profile.range).then_some((
                    unit.id,
                    target,
                    profile.damage_per_second
                        * dt.max(0.0)
                        * if unit.faction == PLAYER {
                            self.modifiers.player_damage_scale
                        } else {
                            1.0
                        },
                ))
            })
            .collect();

        for (attacker, target, amount) in attacks {
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
                self.record(SimulationEventKind::UnitDestroyed {
                    unit_id: target.0,
                    kind,
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
            }
        }
        hasher.write_u64(self.world.selection().ids().len() as u64);
        for id in self.world.selection().ids() {
            hasher.write_u64(u64::from(id.0));
        }
        hasher.write_u64(u64::from(self.resources.amount()));
        hasher.write_u64(u64::from(self.resource_tick.to_bits()));
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

        let first_report = run_trace(&mut first, &trace).unwrap();
        let second_report = run_trace(&mut second, &trace).unwrap();

        assert_eq!(
            first_report.final_state_hash,
            second_report.final_state_hash
        );
        assert_eq!(first_report.commands_applied, 8);
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
    fn movement_without_selection_is_rejected() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation =
            MissionSimulation::from_mission(&mission, SimulationModifiers::default());
        let command = SemanticCommand::new(0, MOVE_SELECTED_ACTION)
            .with_payload(&[0.0_f32, 0.0_f32])
            .unwrap();

        assert!(simulation.apply_command(&command).is_err());
    }
}
