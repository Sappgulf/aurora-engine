//! Reusable real-time-strategy simulation primitives.
//!
//! These types deliberately own gameplay intent without referencing renderer
//! objects: selection, orders, formation destinations, navigation, and fog can
//! therefore run deterministically in fixed update and be saved or tested.

use std::collections::VecDeque;

use glam::{IVec2, Vec2};

use crate::Aabb;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FactionId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnitOrder {
    Idle,
    Move(Vec2),
    Attack(UnitId),
    Interact(Vec2),
    Hold,
}

#[derive(Debug, Clone)]
pub struct RtsUnit {
    pub id: UnitId,
    pub faction: FactionId,
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
    pub speed: f32,
    pub health: f32,
    pub max_health: f32,
    pub order: UnitOrder,
}

impl RtsUnit {
    pub fn new(id: UnitId, faction: FactionId, position: Vec2) -> Self {
        Self {
            id,
            faction,
            position,
            velocity: Vec2::ZERO,
            radius: 28.0,
            speed: 180.0,
            health: 100.0,
            max_health: 100.0,
            order: UnitOrder::Idle,
        }
    }

    pub fn alive(&self) -> bool {
        self.health > 0.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct Selection {
    ids: Vec<UnitId>,
}

impl Selection {
    pub fn ids(&self) -> &[UnitId] {
        &self.ids
    }

    pub fn contains(&self, id: UnitId) -> bool {
        self.ids.contains(&id)
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionBox {
    pub start: Vec2,
    pub current: Vec2,
    pub active: bool,
}

impl SelectionBox {
    pub fn begin(world: Vec2) -> Self {
        Self {
            start: world,
            current: world,
            active: true,
        }
    }

    pub fn update(&mut self, world: Vec2) {
        self.current = world;
    }

    pub fn bounds(self) -> Aabb {
        Aabb::new(self.start, self.current)
    }
}

#[derive(Debug, Default, Clone)]
pub struct RtsWorld {
    units: Vec<RtsUnit>,
    selection: Selection,
    next_id: u32,
}

impl RtsWorld {
    pub fn spawn(&mut self, faction: FactionId, position: Vec2) -> UnitId {
        let id = UnitId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.units.push(RtsUnit::new(id, faction, position));
        id
    }

    pub fn units(&self) -> &[RtsUnit] {
        &self.units
    }

    pub fn units_mut(&mut self) -> &mut [RtsUnit] {
        &mut self.units
    }

    pub fn unit(&self, id: UnitId) -> Option<&RtsUnit> {
        self.units.iter().find(|unit| unit.id == id)
    }

    pub fn unit_mut(&mut self, id: UnitId) -> Option<&mut RtsUnit> {
        self.units.iter_mut().find(|unit| unit.id == id)
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn select_point(&mut self, point: Vec2, faction: FactionId, additive: bool) {
        if !additive {
            self.selection.clear();
        }
        let selected = self
            .units
            .iter()
            .filter(|unit| unit.alive() && unit.faction == faction)
            .filter_map(|unit| {
                let distance = unit.position.distance(point);
                (distance <= unit.radius * 1.35).then_some((unit.id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        if let Some(id) = selected {
            self.add_selected(id);
        }
    }

    pub fn select_bounds(&mut self, bounds: Aabb, faction: FactionId, additive: bool) {
        if !additive {
            self.selection.clear();
        }
        let ids: Vec<UnitId> = self
            .units
            .iter()
            .filter(|unit| {
                unit.alive()
                    && unit.faction == faction
                    && unit.position.x >= bounds.min.x
                    && unit.position.x <= bounds.max.x
                    && unit.position.y >= bounds.min.y
                    && unit.position.y <= bounds.max.y
            })
            .map(|unit| unit.id)
            .collect();
        for id in ids {
            self.add_selected(id);
        }
    }

    fn add_selected(&mut self, id: UnitId) {
        if !self.selection.contains(id) {
            self.selection.ids.push(id);
        }
    }

    /// Issue a move command with deterministic square-spiral formation slots.
    pub fn issue_move(&mut self, destination: Vec2, spacing: f32) {
        let ids = self.selection.ids.clone();
        let width = (ids.len() as f32).sqrt().ceil().max(1.0) as usize;
        for (index, id) in ids.into_iter().enumerate() {
            let column = index % width;
            let row = index / width;
            let centered = Vec2::new(
                column as f32 - (width.saturating_sub(1)) as f32 * 0.5,
                row as f32
                    - self.selection.ids.len().div_ceil(width).saturating_sub(1) as f32 * 0.5,
            );
            if let Some(unit) = self.unit_mut(id) {
                unit.order = UnitOrder::Move(destination + centered * spacing.max(0.0));
            }
        }
    }

    pub fn issue_attack(&mut self, target: UnitId) {
        let ids = self.selection.ids.clone();
        for id in ids {
            if let Some(unit) = self.unit_mut(id) {
                unit.order = UnitOrder::Attack(target);
            }
        }
    }

    pub fn update(&mut self, dt: f32) {
        let positions: Vec<(UnitId, Vec2, bool)> = self
            .units
            .iter()
            .map(|unit| (unit.id, unit.position, unit.alive()))
            .collect();
        for unit in &mut self.units {
            if !unit.alive() {
                unit.velocity = Vec2::ZERO;
                continue;
            }
            let target = match unit.order {
                UnitOrder::Move(target) | UnitOrder::Interact(target) => Some(target),
                UnitOrder::Attack(id) => positions
                    .iter()
                    .find(|(candidate, _, alive)| *candidate == id && *alive)
                    .map(|(_, position, _)| *position),
                UnitOrder::Idle | UnitOrder::Hold => None,
            };
            let Some(target) = target else {
                unit.velocity = Vec2::ZERO;
                continue;
            };
            let offset = target - unit.position;
            if offset.length() <= unit.radius * 0.35 {
                unit.position = target;
                unit.velocity = Vec2::ZERO;
                if matches!(unit.order, UnitOrder::Move(_)) {
                    unit.order = UnitOrder::Idle;
                }
            } else {
                unit.velocity = offset.normalize_or_zero() * unit.speed;
                unit.position += unit.velocity * dt.max(0.0);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct NavGrid {
    width: usize,
    height: usize,
    origin: Vec2,
    cell_size: f32,
    blocked: Vec<bool>,
}

impl NavGrid {
    pub fn new(width: usize, height: usize, origin: Vec2, cell_size: f32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            origin,
            cell_size: cell_size.max(1.0),
            blocked: vec![false; width.max(1) * height.max(1)],
        }
    }

    pub fn world_to_cell(&self, world: Vec2) -> IVec2 {
        ((world - self.origin) / self.cell_size).floor().as_ivec2()
    }

    pub fn cell_center(&self, cell: IVec2) -> Vec2 {
        self.origin + (cell.as_vec2() + Vec2::splat(0.5)) * self.cell_size
    }

    pub fn set_blocked(&mut self, cell: IVec2, blocked: bool) {
        if let Some(index) = self.index(cell) {
            self.blocked[index] = blocked;
        }
    }

    pub fn find_path(&self, start_world: Vec2, goal_world: Vec2) -> Vec<Vec2> {
        let start = self.world_to_cell(start_world);
        let goal = self.world_to_cell(goal_world);
        let (Some(start_index), Some(goal_index)) = (self.index(start), self.index(goal)) else {
            return Vec::new();
        };
        if self.blocked[goal_index] {
            return Vec::new();
        }

        let mut frontier = VecDeque::from([start]);
        let mut came_from = vec![None; self.blocked.len()];
        came_from[start_index] = Some(start);
        while let Some(cell) = frontier.pop_front() {
            if cell == goal {
                break;
            }
            for neighbor in [
                cell + IVec2::X,
                cell - IVec2::X,
                cell + IVec2::Y,
                cell - IVec2::Y,
            ] {
                let Some(index) = self.index(neighbor) else {
                    continue;
                };
                if self.blocked[index] || came_from[index].is_some() {
                    continue;
                }
                came_from[index] = Some(cell);
                frontier.push_back(neighbor);
            }
        }
        if came_from[goal_index].is_none() {
            return Vec::new();
        }
        let mut cells = vec![goal];
        let mut current = goal;
        while current != start {
            let Some(previous) = came_from[self.index(current).unwrap()] else {
                return Vec::new();
            };
            current = previous;
            cells.push(current);
        }
        cells.reverse();
        cells
            .into_iter()
            .skip(1)
            .map(|cell| self.cell_center(cell))
            .collect()
    }

    fn index(&self, cell: IVec2) -> Option<usize> {
        if cell.x < 0
            || cell.y < 0
            || cell.x as usize >= self.width
            || cell.y as usize >= self.height
        {
            return None;
        }
        Some(cell.y as usize * self.width + cell.x as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FogState {
    #[default]
    Hidden,
    Explored,
    Visible,
}

#[derive(Debug, Clone)]
pub struct FogOfWar {
    width: usize,
    height: usize,
    origin: Vec2,
    cell_size: f32,
    cells: Vec<FogState>,
}

impl FogOfWar {
    pub fn new(width: usize, height: usize, origin: Vec2, cell_size: f32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            origin,
            cell_size: cell_size.max(1.0),
            cells: vec![FogState::Hidden; width.max(1) * height.max(1)],
        }
    }

    pub fn begin_frame(&mut self) {
        for state in &mut self.cells {
            if *state == FogState::Visible {
                *state = FogState::Explored;
            }
        }
    }

    pub fn reveal(&mut self, world: Vec2, radius: f32) {
        let center = ((world - self.origin) / self.cell_size).floor().as_ivec2();
        let cells = (radius.max(0.0) / self.cell_size).ceil() as i32;
        for y in -cells..=cells {
            for x in -cells..=cells {
                let cell = center + IVec2::new(x, y);
                if Vec2::new(x as f32, y as f32).length() > cells as f32 {
                    continue;
                }
                if cell.x >= 0
                    && cell.y >= 0
                    && (cell.x as usize) < self.width
                    && (cell.y as usize) < self.height
                {
                    self.cells[cell.y as usize * self.width + cell.x as usize] = FogState::Visible;
                }
            }
        }
    }

    pub fn state_at(&self, world: Vec2) -> FogState {
        let cell = ((world - self.origin) / self.cell_size).floor().as_ivec2();
        if cell.x < 0
            || cell.y < 0
            || cell.x as usize >= self.width
            || cell.y as usize >= self.height
        {
            return FogState::Hidden;
        }
        self.cells[cell.y as usize * self.width + cell.x as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: FactionId = FactionId(1);

    #[test]
    fn box_selection_and_formation_orders_are_deterministic() {
        let mut world = RtsWorld::default();
        world.spawn(PLAYER, Vec2::new(-10.0, 0.0));
        world.spawn(PLAYER, Vec2::new(10.0, 0.0));
        world.spawn(FactionId(2), Vec2::ZERO);
        world.select_bounds(
            Aabb::from_center_size(Vec2::ZERO, Vec2::splat(100.0)),
            PLAYER,
            false,
        );
        assert_eq!(world.selection().ids().len(), 2);
        world.issue_move(Vec2::new(200.0, 100.0), 40.0);
        let destinations: Vec<Vec2> = world
            .units()
            .iter()
            .filter_map(|unit| match unit.order {
                UnitOrder::Move(destination) => Some(destination),
                _ => None,
            })
            .collect();
        assert_eq!(
            destinations,
            [Vec2::new(180.0, 100.0), Vec2::new(220.0, 100.0)]
        );
    }

    #[test]
    fn navigation_routes_around_blocked_cells() {
        let mut grid = NavGrid::new(5, 3, Vec2::ZERO, 10.0);
        grid.set_blocked(IVec2::new(2, 1), true);
        let path = grid.find_path(Vec2::new(5.0, 15.0), Vec2::new(45.0, 15.0));
        assert!(!path.is_empty());
        assert!(path
            .iter()
            .all(|point| grid.world_to_cell(*point) != IVec2::new(2, 1)));
    }

    #[test]
    fn fog_transitions_from_visible_to_explored() {
        let mut fog = FogOfWar::new(8, 8, Vec2::ZERO, 10.0);
        fog.reveal(Vec2::new(25.0, 25.0), 12.0);
        assert_eq!(fog.state_at(Vec2::new(25.0, 25.0)), FogState::Visible);
        fog.begin_frame();
        assert_eq!(fog.state_at(Vec2::new(25.0, 25.0)), FogState::Explored);
    }
}
