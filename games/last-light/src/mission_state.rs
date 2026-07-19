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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    Relay(usize),
    Fabricator,
    Reactor,
}

impl StructureKind {
    pub const RELAY_RADIUS: f32 = 85.0;
    pub const FABRICATOR_RADIUS: f32 = 105.0;
    pub const REACTOR_RADIUS: f32 = 135.0;
}
