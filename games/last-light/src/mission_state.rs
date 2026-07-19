//! Resettable state owned by an active Last Light mission.

use glam::Vec2;

pub struct Relay {
    pub position: Vec2,
    pub progress: f32,
    pub active: bool,
}
pub struct FieldBeacon {
    pub position: Vec2,
}

pub struct SalvageNode {
    pub position: Vec2,
    pub remaining: u32,
    pub harvest_buffer: f32,
    pub kind: ResourceKind,
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
