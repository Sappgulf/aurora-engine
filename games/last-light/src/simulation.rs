//! Renderer-free tactical state for Last Light missions.

use std::collections::{HashMap, VecDeque};

use aurora_engine::{
    mark_obstacles, Aabb, DeterministicSimulation, FactionId, NavGrid, RtsWorld, SemanticCommand,
    StableStateHasher, StateHash, UnitId, UnitOrder,
};
use glam::Vec2;

use crate::missions::MissionDef;
use crate::units::{UnitKind, CHOIR, PLAYER};

pub const MAP_SIZE: Vec2 = Vec2::new(2600.0, 1460.0);
pub const NAV_CELL_SIZE: f32 = 40.0;
const DEFAULT_FIXED_TICK_HZ: u32 = 60;

pub const SELECT_ALL_ACTION: &str = "last_light.select_all";
pub const MOVE_SELECTED_ACTION: &str = "last_light.move_selected";

#[derive(Debug, Clone, Copy)]
pub struct SpawnModifiers {
    pub player_health: f32,
    pub player_speed: f32,
}

impl Default for SpawnModifiers {
    fn default() -> Self {
        Self {
            player_health: 1.0,
            player_speed: 1.0,
        }
    }
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
    tick: u64,
    fixed_dt: f32,
}

impl MissionSimulation {
    pub fn from_mission(mission: &MissionDef, modifiers: SpawnModifiers) -> Self {
        let mut nav = NavGrid::new(
            (MAP_SIZE.x / NAV_CELL_SIZE).ceil() as usize,
            (MAP_SIZE.y / NAV_CELL_SIZE).ceil() as usize,
            -MAP_SIZE * 0.5,
            NAV_CELL_SIZE,
        );
        mark_obstacles(&mut nav, &mission.obstacles);
        let mut simulation = Self {
            world: RtsWorld::default(),
            kinds: HashMap::new(),
            nav,
            player_paths: HashMap::new(),
            escort_unit: None,
            tick: 0,
            fixed_dt: 1.0 / DEFAULT_FIXED_TICK_HZ as f32,
        };
        for spawn in &mission.player_spawns {
            let id = simulation.spawn(
                spawn.kind,
                PLAYER,
                spawn.position,
                spawn.health,
                spawn.speed,
                modifiers,
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
                modifiers,
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
        modifiers: SpawnModifiers,
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

    pub fn issue_move_order(&mut self, destination: Vec2) {
        let selected_ids = self.world.selection().ids().to_vec();
        self.world.issue_move(destination, 74.0);
        self.route_around_obstacles(&selected_ids);
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

    pub fn fixed_step_with_dt(&mut self, dt: f32) {
        self.world.update(dt);
        self.advance_player_paths();
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
}

impl DeterministicSimulation for MissionSimulation {
    fn apply_command(&mut self, command: &SemanticCommand) -> Result<(), String> {
        match command.action.as_str() {
            SELECT_ALL_ACTION => {
                self.select_all_player_units();
                Ok(())
            }
            MOVE_SELECTED_ACTION if !self.world.selection().ids().is_empty() => {
                self.issue_move_order(Self::command_destination(command)?);
                Ok(())
            }
            MOVE_SELECTED_ACTION => Err("move requires a selected Lantern unit".to_owned()),
            action => Err(format!("unknown Last Light action {action}")),
        }
    }

    fn fixed_step(&mut self) {
        self.fixed_step_with_dt(self.fixed_dt);
    }

    fn state_hash(&self) -> StateHash {
        let mut hasher = StableStateHasher::new();
        hasher.write_u64(self.tick);
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
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use aurora_engine::{run_trace, AuroraTrace, SemanticCommand};

    use super::*;

    fn selection_and_move_trace() -> AuroraTrace {
        let mut trace = AuroraTrace::new("last_light.reclaim.selection_move", 44117, 60, 180);
        trace.push(SemanticCommand::new(1, SELECT_ALL_ACTION));
        trace.push(
            SemanticCommand::new(5, MOVE_SELECTED_ACTION)
                .with_payload(&[-560.0_f32, -120.0_f32])
                .unwrap(),
        );
        trace
    }

    #[test]
    fn reclaim_selection_and_move_replays_to_the_same_hash() {
        let mission = crate::missions::reclaim_the_reactor();
        let trace = selection_and_move_trace();
        let mut first = MissionSimulation::from_mission(&mission, SpawnModifiers::default());
        let mut second = MissionSimulation::from_mission(&mission, SpawnModifiers::default());
        let starting_positions: Vec<_> = first
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER)
            .map(|unit| unit.position)
            .collect();

        let first_report = run_trace(&mut first, &trace).unwrap();
        let second_report = run_trace(&mut second, &trace).unwrap();

        assert_eq!(
            first_report.final_state_hash,
            second_report.final_state_hash
        );
        assert_eq!(first_report.commands_applied, 2);
        assert_eq!(first.world.selection().ids().len(), 3);
        assert!(
            first
                .world
                .units()
                .iter()
                .filter(|unit| unit.faction == PLAYER)
                .zip(starting_positions)
                .all(|(unit, start)| unit.position.distance(start) > 50.0),
            "every selected Lantern unit should visibly advance"
        );
    }

    #[test]
    fn movement_without_selection_is_rejected() {
        let mission = crate::missions::reclaim_the_reactor();
        let mut simulation = MissionSimulation::from_mission(&mission, SpawnModifiers::default());
        let command = SemanticCommand::new(0, MOVE_SELECTED_ACTION)
            .with_payload(&[0.0_f32, 0.0_f32])
            .unwrap();

        assert!(simulation.apply_command(&command).is_err());
    }
}
