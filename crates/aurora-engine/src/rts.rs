//! Reusable real-time-strategy simulation primitives.
//!
//! These types deliberately own gameplay intent without referencing renderer
//! objects: selection, orders, formation destinations, navigation, and fog can
//! therefore run deterministically in fixed update and be saved or tested.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};

use glam::{IVec2, Vec2};

use crate::Aabb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementError {
    OutsideBuildArea,
    TooFarFromPower,
    Obstructed,
}

#[derive(Debug, Clone)]
pub struct PlacementRules {
    pub build_area: Aabb,
    pub power_sources: Vec<Vec2>,
    pub obstructions: Vec<(Vec2, f32)>,
    pub max_power_distance: f32,
}

impl PlacementRules {
    pub fn validate(&self, position: Vec2, radius: f32) -> Result<(), PlacementError> {
        let radius = radius.max(0.0);
        if position.x - radius < self.build_area.min.x
            || position.x + radius > self.build_area.max.x
            || position.y - radius < self.build_area.min.y
            || position.y + radius > self.build_area.max.y
        {
            return Err(PlacementError::OutsideBuildArea);
        }
        if !self.power_sources.is_empty()
            && !self
                .power_sources
                .iter()
                .any(|source| source.distance(position) <= self.max_power_distance.max(0.0))
        {
            return Err(PlacementError::TooFarFromPower);
        }
        if self
            .obstructions
            .iter()
            .any(|(center, obstruction_radius)| {
                center.distance(position) < radius + obstruction_radius.max(0.0)
            })
        {
            return Err(PlacementError::Obstructed);
        }
        Ok(())
    }
}

/// Converts between authored world coordinates and a rectangular tactical map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapTransform {
    pub world: Aabb,
    pub panel: Aabb,
}

impl MinimapTransform {
    pub fn world_to_panel(self, position: Vec2) -> Vec2 {
        let world_size = self.world.size().max(Vec2::splat(f32::EPSILON));
        let normalized = ((position - self.world.min) / world_size).clamp(Vec2::ZERO, Vec2::ONE);
        self.panel.min + normalized * self.panel.size()
    }

    pub fn panel_to_world(self, position: Vec2) -> Option<Vec2> {
        if position.x < self.panel.min.x
            || position.x > self.panel.max.x
            || position.y < self.panel.min.y
            || position.y > self.panel.max.y
        {
            return None;
        }
        let normalized =
            (position - self.panel.min) / self.panel.size().max(Vec2::splat(f32::EPSILON));
        Some(self.world.min + normalized * self.world.size())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProductId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionRecipe {
    pub product: ProductId,
    pub cost: u32,
    pub build_millis: u32,
}

impl ProductionRecipe {
    pub const fn new(product: ProductId, cost: u32, build_millis: u32) -> Self {
        Self {
            product,
            cost,
            build_millis,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    InsufficientResources,
    Full,
}

/// Admission failures for the supply-aware production path. This is kept
/// separate from [`QueueError`] so existing games that exhaustively match the
/// original queue API remain source-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplyQueueError {
    InsufficientResources,
    InsufficientSupply,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceBank {
    amount: u32,
}

impl ResourceBank {
    pub const fn new(amount: u32) -> Self {
        Self { amount }
    }

    pub const fn amount(self) -> u32 {
        self.amount
    }

    pub fn credit(&mut self, amount: u32) {
        self.amount = self.amount.saturating_add(amount);
    }

    pub fn spend(&mut self, amount: u32) -> bool {
        if self.amount < amount {
            return false;
        }
        self.amount -= amount;
        true
    }
}

/// A deterministic two-channel resource wallet for RTS economies. `primary`
/// is the common construction currency; `secondary` is intentionally generic
/// so games can name it salvage, gas, flux, or another strategic material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceSet {
    pub primary: u32,
    pub secondary: u32,
}

impl ResourceSet {
    pub const fn new(primary: u32, secondary: u32) -> Self {
        Self { primary, secondary }
    }

    pub const fn can_afford(self, cost: ResourceCost) -> bool {
        self.primary >= cost.primary && self.secondary >= cost.secondary
    }

    pub fn credit(&mut self, amount: ResourceCost) {
        self.primary = self.primary.saturating_add(amount.primary);
        self.secondary = self.secondary.saturating_add(amount.secondary);
    }

    pub fn spend(&mut self, cost: ResourceCost) -> bool {
        if !self.can_afford(cost) {
            return false;
        }
        self.primary -= cost.primary;
        self.secondary -= cost.secondary;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceCost {
    pub primary: u32,
    pub secondary: u32,
}

impl ResourceCost {
    pub const fn new(primary: u32, secondary: u32) -> Self {
        Self { primary, secondary }
    }
}

/// Supply is reserved when a unit enters a production queue, preventing a
/// player from over-queuing units and discovering the cap only on deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SupplyLedger {
    used: u32,
    capacity: u32,
}

impl SupplyLedger {
    pub const fn new(capacity: u32) -> Self {
        Self { used: 0, capacity }
    }

    pub const fn used(self) -> u32 {
        self.used
    }

    pub const fn capacity(self) -> u32 {
        self.capacity
    }

    pub const fn available(self) -> u32 {
        self.capacity.saturating_sub(self.used)
    }

    pub fn set_capacity(&mut self, capacity: u32) {
        self.capacity = capacity;
    }

    pub fn try_add(&mut self, amount: u32) -> bool {
        if self.available() < amount {
            return false;
        }
        self.used += amount;
        true
    }

    pub fn release(&mut self, amount: u32) {
        self.used = self.used.saturating_sub(amount);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TechId(pub u16);

/// A compact, data-driven prerequisite graph. Vectors keep iteration order
/// stable for deterministic replays and save migration.
#[derive(Debug, Clone, Default)]
pub struct TechGraph {
    unlocked: Vec<TechId>,
    prerequisites: Vec<(TechId, Vec<TechId>)>,
}

impl TechGraph {
    pub fn define(&mut self, tech: TechId, prerequisites: impl Into<Vec<TechId>>) {
        if let Some(entry) = self
            .prerequisites
            .iter_mut()
            .find(|(defined, _)| *defined == tech)
        {
            entry.1 = prerequisites.into();
        } else {
            self.prerequisites.push((tech, prerequisites.into()));
        }
    }

    pub fn is_unlocked(&self, tech: TechId) -> bool {
        self.unlocked.contains(&tech)
    }

    pub fn can_unlock(&self, tech: TechId) -> bool {
        !self.is_unlocked(tech)
            && self
                .prerequisites
                .iter()
                .find(|(defined, _)| *defined == tech)
                .is_none_or(|(_, requirements)| {
                    requirements
                        .iter()
                        .all(|requirement| self.is_unlocked(*requirement))
                })
    }

    pub fn unlock(&mut self, tech: TechId) -> bool {
        if !self.can_unlock(tech) {
            return false;
        }
        self.unlocked.push(tech);
        true
    }

    pub fn unlocked(&self) -> &[TechId] {
        &self.unlocked
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageType {
    Normal,
    Concussive,
    Explosive,
    Energy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmorClass {
    Small,
    Medium,
    Large,
    Structure,
}

impl DamageType {
    pub const fn multiplier(self, armor_class: ArmorClass) -> f32 {
        match (self, armor_class) {
            (Self::Concussive, ArmorClass::Medium) => 0.5,
            (Self::Concussive, ArmorClass::Large) => 0.25,
            (Self::Concussive, ArmorClass::Structure) => 0.35,
            (Self::Explosive, ArmorClass::Small) => 0.5,
            (Self::Explosive, ArmorClass::Medium) => 0.75,
            (Self::Explosive, ArmorClass::Structure) => 0.75,
            _ => 1.0,
        }
    }
}

/// Maximum cover fraction recognized by the tactical resolver.
pub const TERRAIN_MAX_COVER: f32 = 0.3;

/// Cover at or above this threshold is a meaningful covered pocket for
/// tactical overlays and map-authoring previews.
pub const TERRAIN_COVER_THRESHOLD: f32 = 0.2;

/// Coarse strategic classification used by minimaps, terrain overlays, and
/// authoring tools. The class intentionally stays independent of art assets:
/// callers can choose their own palette or iconography for each class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainClass {
    Open,
    Covered,
    HighGround,
    FortifiedHighGround,
}

impl TerrainClass {
    pub const fn has_cover(self) -> bool {
        matches!(self, Self::Covered | Self::FortifiedHighGround)
    }

    pub const fn is_high_ground(self) -> bool {
        matches!(self, Self::HighGround | Self::FortifiedHighGround)
    }
}

/// Stable, renderer-neutral terrain data for a minimap marker or context
/// chip. Keeping cover as an integer percent makes this payload cheap to
/// compare, serialize, and display in native and browser builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerrainReadout {
    pub class: TerrainClass,
    pub elevation: i8,
    pub cover_percent: u8,
}

impl TerrainReadout {
    pub const fn has_cover(self) -> bool {
        self.class.has_cover()
    }

    pub const fn is_high_ground(self) -> bool {
        self.class.is_high_ground()
    }
}

/// Authored strategic map metadata used by combat and future visibility or
/// movement rules. `cover` is a 0..1 fraction; the engine clamps it to the
/// [`TERRAIN_MAX_COVER`] contract when classifying or resolving damage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainZone {
    pub bounds: Aabb,
    pub elevation: i8,
    pub cover: f32,
}

impl TerrainZone {
    pub const fn new(bounds: Aabb, elevation: i8, cover: f32) -> Self {
        Self {
            bounds,
            elevation,
            cover,
        }
    }

    pub fn contains(self, position: Vec2) -> bool {
        self.bounds.contains_point(position)
    }

    /// Returns a finite cover value suitable for rendering or combat.
    ///
    /// Mission validators reject malformed values, but keeping this guard in
    /// the engine prevents editor previews or runtime-authored maps from
    /// turning a NaN/oversized cover value into non-deterministic colors or
    /// damage.
    pub fn normalized_cover(self) -> f32 {
        if self.cover.is_finite() {
            self.cover.clamp(0.0, TERRAIN_MAX_COVER)
        } else {
            0.0
        }
    }

    /// Classifies this zone for a compact strategic overlay or authoring UI.
    pub fn classification(self) -> TerrainClass {
        match (
            self.elevation > 0,
            self.normalized_cover() >= TERRAIN_COVER_THRESHOLD,
        ) {
            (false, false) => TerrainClass::Open,
            (false, true) => TerrainClass::Covered,
            (true, false) => TerrainClass::HighGround,
            (true, true) => TerrainClass::FortifiedHighGround,
        }
    }

    /// Produces the compact payload used by tactical overlays and context
    /// chips without exposing the zone's world-space bounds.
    pub fn readout(self) -> TerrainReadout {
        TerrainReadout {
            class: self.classification(),
            elevation: self.elevation,
            cover_percent: (self.normalized_cover() * 100.0).round() as u8,
        }
    }

    /// Returns a stable priority for resolving overlapping authored zones.
    ///
    /// Elevation is the primary tactical discriminator, followed by cover.
    /// Cover is quantized to thousandths so callers can compare priorities
    /// without relying on floating-point ordering or NaN behavior.
    pub fn priority_key(self) -> (i16, u16) {
        (
            i16::from(self.elevation),
            (self.normalized_cover() * 1_000.0).round() as u16,
        )
    }

    /// Resolves the strongest authored zone at a world position.
    ///
    /// The returned index lets renderers/editor tools keep a stable link to
    /// the original authoring entry. Higher elevation wins, then stronger
    /// cover; equal-priority zones keep the lowest authored index so adding a
    /// later decorative band cannot silently change an existing result.
    pub fn resolve_at(position: Vec2, zones: &[Self]) -> Option<(usize, Self)> {
        zones
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, zone)| zone.contains(position))
            .max_by(|(left_index, left), (right_index, right)| {
                left.priority_key()
                    .cmp(&right.priority_key())
                    .then_with(|| right_index.cmp(left_index))
            })
    }

    /// Resolves only the compact terrain payload needed by a HUD or minimap.
    pub fn resolve_readout_at(position: Vec2, zones: &[Self]) -> Option<(usize, TerrainReadout)> {
        Self::resolve_at(position, zones).map(|(index, zone)| (index, zone.readout()))
    }

    pub fn damage_multiplier(self, attacker_elevation: i8) -> f32 {
        let high_ground = if attacker_elevation < self.elevation {
            0.7
        } else {
            1.0
        };
        high_ground * (1.0 - self.normalized_cover())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProductionItem {
    pub product: ProductId,
    pub remaining_seconds: f32,
    pub total_seconds: f32,
}

impl ProductionItem {
    pub fn progress(self) -> f32 {
        if self.total_seconds <= f32::EPSILON {
            1.0
        } else {
            (1.0 - self.remaining_seconds / self.total_seconds).clamp(0.0, 1.0)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProductionReservation {
    resource_cost: u32,
    supply_cost: u32,
}

/// Failure reasons for cancelling a production item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionCancelError {
    InvalidIndex,
    SupplyLedgerRequired,
}

/// Stable result returned after a production item is cancelled. The game can
/// use this receipt for HUD feedback without reconstructing queue metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionCancelReceipt {
    pub product: ProductId,
    pub refunded_resources: u32,
    pub released_supply: u32,
}

#[derive(Debug, Clone)]
pub struct ProductionQueue {
    items: VecDeque<ProductionItem>,
    reservations: VecDeque<ProductionReservation>,
    capacity: usize,
}

impl ProductionQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: VecDeque::new(),
            reservations: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn items(&self) -> &VecDeque<ProductionItem> {
        &self.items
    }

    pub fn enqueue(
        &mut self,
        recipe: ProductionRecipe,
        resources: &mut ResourceBank,
    ) -> Result<(), QueueError> {
        if self.items.len() >= self.capacity {
            return Err(QueueError::Full);
        }
        if !resources.spend(recipe.cost) {
            return Err(QueueError::InsufficientResources);
        }
        self.push_recipe(recipe, 0);
        Ok(())
    }

    /// Enqueues a unit while atomically spending resources and reserving its
    /// supply. Supply is reserved at queue admission, matching RTS behavior:
    /// a player cannot over-queue units and discover the cap only on spawn.
    /// Existing [`Self::enqueue`] remains available for games without supply.
    pub fn enqueue_with_supply(
        &mut self,
        recipe: ProductionRecipe,
        resources: &mut ResourceBank,
        supply: &mut SupplyLedger,
        supply_cost: u32,
    ) -> Result<(), SupplyQueueError> {
        if self.items.len() >= self.capacity {
            return Err(SupplyQueueError::Full);
        }
        if resources.amount() < recipe.cost {
            return Err(SupplyQueueError::InsufficientResources);
        }
        if supply.available() < supply_cost {
            return Err(SupplyQueueError::InsufficientSupply);
        }

        // The preflight checks above make these operations infallible in the
        // single-threaded simulation, while the rollback keeps the contract
        // correct even if a future ResourceBank/SupplyLedger implementation
        // adds additional admission rules.
        if !supply.try_add(supply_cost) {
            return Err(SupplyQueueError::InsufficientSupply);
        }
        if !resources.spend(recipe.cost) {
            supply.release(supply_cost);
            return Err(SupplyQueueError::InsufficientResources);
        }
        self.push_recipe(recipe, supply_cost);
        Ok(())
    }

    fn push_recipe(&mut self, recipe: ProductionRecipe, supply_cost: u32) {
        let seconds = recipe.build_millis as f32 / 1000.0;
        self.items.push_back(ProductionItem {
            product: recipe.product,
            remaining_seconds: seconds,
            total_seconds: seconds,
        });
        self.reservations.push_back(ProductionReservation {
            resource_cost: recipe.cost,
            supply_cost,
        });
    }

    /// Cancels a legacy production item and refunds the requested percentage
    /// of its original resource cost. Supply-backed items must use
    /// [`Self::cancel_with_supply`] so a reserved cap cannot leak.
    pub fn cancel(
        &mut self,
        index: usize,
        resources: &mut ResourceBank,
        refund_percent: u8,
    ) -> Result<ProductionCancelReceipt, ProductionCancelError> {
        self.cancel_internal(index, resources, None, refund_percent)
    }

    /// Cancels a supply-aware production item, refunds its deterministic cost
    /// percentage, and releases the reserved supply in the same operation.
    pub fn cancel_with_supply(
        &mut self,
        index: usize,
        resources: &mut ResourceBank,
        supply: &mut SupplyLedger,
        refund_percent: u8,
    ) -> Result<ProductionCancelReceipt, ProductionCancelError> {
        self.cancel_internal(index, resources, Some(supply), refund_percent)
    }

    fn cancel_internal(
        &mut self,
        index: usize,
        resources: &mut ResourceBank,
        mut supply: Option<&mut SupplyLedger>,
        refund_percent: u8,
    ) -> Result<ProductionCancelReceipt, ProductionCancelError> {
        let Some(reservation) = self.reservations.get(index).copied() else {
            return Err(ProductionCancelError::InvalidIndex);
        };
        if reservation.supply_cost > 0 && supply.is_none() {
            return Err(ProductionCancelError::SupplyLedgerRequired);
        }

        let Some(item) = self.items.remove(index) else {
            return Err(ProductionCancelError::InvalidIndex);
        };
        let Some(reservation) = self.reservations.remove(index) else {
            // The two deques are kept in lockstep by the queue's private
            // helpers. Treat a broken invariant as an invalid cancellation
            // rather than refunding against an unknown cost.
            self.items.insert(index, item);
            self.reservations.insert(index, reservation);
            return Err(ProductionCancelError::InvalidIndex);
        };
        let refund_percent = u64::from(refund_percent.min(100));
        let refunded_resources = ((u64::from(reservation.resource_cost) * refund_percent) / 100)
            .min(u64::from(u32::MAX)) as u32;
        resources.credit(refunded_resources);
        if let Some(supply) = supply.as_mut() {
            supply.release(reservation.supply_cost);
        }
        Ok(ProductionCancelReceipt {
            product: item.product,
            refunded_resources,
            released_supply: reservation.supply_cost,
        })
    }

    /// Advances only the front item and returns all products completed this tick.
    pub fn update(&mut self, mut dt: f32) -> Vec<ProductId> {
        let mut completed = Vec::new();
        dt = dt.max(0.0);
        while dt > 0.0 {
            let Some(front) = self.items.front_mut() else {
                break;
            };
            if front.remaining_seconds > dt {
                front.remaining_seconds -= dt;
                break;
            }
            dt -= front.remaining_seconds.max(0.0);
            if let Some(item) = self.items.pop_front() {
                self.reservations.pop_front();
                completed.push(item.product);
            }
        }
        completed
    }
}

impl Default for ProductionQueue {
    fn default() -> Self {
        Self::new(5)
    }
}

/// Stable identifier for a structure or infrastructure build job. Games can
/// map this to a structure kind without making the engine know about their
/// content roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuildId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildRecipe {
    pub build: BuildId,
    pub build_seconds: f32,
}

impl BuildRecipe {
    pub const fn new(build: BuildId, build_seconds: f32) -> Self {
        Self {
            build,
            build_seconds,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildQueueError {
    Full,
}

#[derive(Debug, Clone)]
struct SpatialUnitIndex {
    cell_size: f32,
    buckets: HashMap<(i32, i32), Vec<UnitId>>,
    alive_units: HashSet<UnitId>,
}

impl SpatialUnitIndex {
    fn new(cell_size: f32) -> Self {
        Self {
            cell_size: cell_size.max(1.0),
            buckets: HashMap::new(),
            alive_units: HashSet::new(),
        }
    }

    fn clear(&mut self) {
        self.buckets.clear();
        self.alive_units.clear();
    }

    fn build(&mut self, units: &[RtsUnit]) {
        self.buckets.clear();
        self.alive_units.clear();

        for unit in units {
            if !unit.alive() {
                continue;
            }
            let cell = self.cell(unit.position);
            self.buckets.entry(cell).or_default().push(unit.id);
            self.alive_units.insert(unit.id);
        }

        for bucket in self.buckets.values_mut() {
            bucket.sort_by_key(|id| id.0);
        }
    }

    fn cell(&self, position: Vec2) -> (i32, i32) {
        let scaled = position / self.cell_size;
        (scaled.x.floor() as i32, scaled.y.floor() as i32)
    }

    fn query_aabb_ids(&self, bounds: Aabb) -> Vec<UnitId> {
        let min_cell = self.cell(bounds.min);
        let max_cell = self.cell(bounds.max);

        if min_cell > max_cell {
            return Vec::new();
        }

        let mut ids = Vec::new();
        for y in min_cell.1..=max_cell.1 {
            for x in min_cell.0..=max_cell.0 {
                if let Some(bucket) = self.buckets.get(&(x, y)) {
                    ids.extend_from_slice(bucket);
                }
            }
        }
        ids
    }

    fn query_cell_ids(&self, position: Vec2, radius: f32) -> Vec<UnitId> {
        let radius = radius.max(0.0);
        if radius.is_infinite() {
            let mut all: Vec<UnitId> = self.alive_units.iter().copied().collect();
            all.sort_by_key(|id| id.0);
            return all;
        }

        let min = position - Vec2::splat(radius);
        let max = position + Vec2::splat(radius);
        let min_cell = self.cell(min);
        let max_cell = self.cell(max);

        if min_cell > max_cell {
            return Vec::new();
        }

        let mut ids = Vec::new();
        for y in min_cell.1..=max_cell.1 {
            for x in min_cell.0..=max_cell.0 {
                if let Some(bucket) = self.buckets.get(&(x, y)) {
                    ids.extend_from_slice(bucket);
                }
            }
        }
        ids
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildItem {
    pub build: BuildId,
    pub remaining_seconds: f32,
    pub total_seconds: f32,
}

impl BuildItem {
    pub fn progress(self) -> f32 {
        if self.total_seconds <= f32::EPSILON {
            1.0
        } else {
            (1.0 - self.remaining_seconds / self.total_seconds).clamp(0.0, 1.0)
        }
    }
}

/// A deterministic, renderer-free queue for infrastructure jobs. Unlike
/// [`ProductionQueue`], it intentionally does not own a resource wallet:
/// games can validate power, tech, and multi-resource costs before enqueueing
/// a build while sharing the same bounded timing semantics.
#[derive(Debug, Clone)]
pub struct BuildQueue {
    items: VecDeque<BuildItem>,
    capacity: usize,
}

impl BuildQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn items(&self) -> &VecDeque<BuildItem> {
        &self.items
    }

    pub fn front(&self) -> Option<&BuildItem> {
        self.items.front()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.items.len() >= self.capacity
    }

    pub fn enqueue(&mut self, recipe: BuildRecipe) -> Result<(), BuildQueueError> {
        if self.is_full() {
            return Err(BuildQueueError::Full);
        }
        let seconds = recipe.build_seconds.max(0.0);
        self.items.push_back(BuildItem {
            build: recipe.build,
            remaining_seconds: seconds,
            total_seconds: seconds,
        });
        Ok(())
    }

    /// Advances only the front job and returns every build completed during
    /// this tick. Any elapsed time beyond one completion carries into the
    /// next queued job, keeping large fixed-step updates deterministic.
    pub fn update(&mut self, mut dt: f32) -> Vec<BuildId> {
        let mut completed = Vec::new();
        dt = dt.max(0.0);
        while let Some(front) = self.items.front_mut() {
            if front.remaining_seconds > dt {
                front.remaining_seconds -= dt;
                break;
            }
            dt -= front.remaining_seconds.max(0.0);
            if let Some(item) = self.items.pop_front() {
                completed.push(item.build);
            }
        }
        completed
    }
}

impl Default for BuildQueue {
    fn default() -> Self {
        Self::new(3)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PowerNodeId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerNode {
    pub id: PowerNodeId,
    pub supply: u32,
    pub demand: u32,
    pub online: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PowerGrid {
    nodes: Vec<PowerNode>,
    links: Vec<(PowerNodeId, PowerNodeId)>,
}

impl PowerGrid {
    pub fn add_node(&mut self, node: PowerNode) {
        if let Some(existing) = self
            .nodes
            .iter_mut()
            .find(|existing| existing.id == node.id)
        {
            *existing = node;
        } else {
            self.nodes.push(node);
        }
    }

    pub fn set_online(&mut self, id: PowerNodeId, online: bool) {
        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
            node.online = online;
        }
    }

    pub fn link(&mut self, a: PowerNodeId, b: PowerNodeId) {
        if a != b
            && !self
                .links
                .iter()
                .any(|&(left, right)| (left == a && right == b) || (left == b && right == a))
        {
            self.links.push((a, b));
        }
    }

    pub fn is_powered(&self, id: PowerNodeId) -> bool {
        let Some(start) = self.nodes.iter().find(|node| node.id == id && node.online) else {
            return false;
        };
        let mut frontier = VecDeque::from([start.id]);
        let mut visited = Vec::new();
        while let Some(current) = frontier.pop_front() {
            if visited.contains(&current) {
                continue;
            }
            let Some(node) = self
                .nodes
                .iter()
                .find(|node| node.id == current && node.online)
            else {
                continue;
            };
            visited.push(node.id);
            for &(a, b) in &self.links {
                if a == current && !visited.contains(&b) {
                    frontier.push_back(b);
                } else if b == current && !visited.contains(&a) {
                    frontier.push_back(a);
                }
            }
        }
        let (supply, demand) = visited.iter().fold((0_u32, 0_u32), |totals, id| {
            let node = self.nodes.iter().find(|node| node.id == *id).unwrap();
            (
                totals.0.saturating_add(node.supply),
                totals.1.saturating_add(node.demand),
            )
        });
        supply > 0 && supply >= demand
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FactionId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnitOrder {
    Idle,
    Move(Vec2),
    AttackMove(Vec2),
    Attack(UnitId),
    Patrol(Vec2, Vec2),
    Follow(UnitId),
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
    /// Optional center-to-center firing distance for explicit `Attack`
    /// orders. A zero value preserves the legacy behavior of walking all the
    /// way to the target, which keeps non-combat users source-compatible.
    /// Combat games should set this to their weapon range (or a small margin
    /// below it) so units stop at a readable firing line instead of stacking
    /// on the target's origin.
    pub engagement_range: f32,
    pub speed: f32,
    pub health: f32,
    pub max_health: f32,
    pub order: UnitOrder,
    pub queued_orders: VecDeque<UnitOrder>,
}

impl RtsUnit {
    pub fn new(id: UnitId, faction: FactionId, position: Vec2) -> Self {
        Self {
            id,
            faction,
            position,
            velocity: Vec2::ZERO,
            radius: 28.0,
            engagement_range: 0.0,
            speed: 180.0,
            health: 100.0,
            max_health: 100.0,
            order: UnitOrder::Idle,
            queued_orders: VecDeque::new(),
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

#[derive(Debug, Clone)]
pub struct RtsWorld {
    units: Vec<RtsUnit>,
    selection: Selection,
    control_groups: [Vec<UnitId>; 10],
    next_id: u32,
    spatial_index: RefCell<SpatialUnitIndex>,
    spatial_dirty: Cell<bool>,
}

impl Default for RtsWorld {
    fn default() -> Self {
        Self {
            units: Vec::new(),
            selection: Selection::default(),
            control_groups: std::array::from_fn(|_| Vec::new()),
            next_id: 0,
            spatial_index: RefCell::new(SpatialUnitIndex::new(Self::UNIT_SPATIAL_CELL_SIZE)),
            spatial_dirty: Cell::new(true),
        }
    }
}

impl RtsWorld {
    const UNIT_SPATIAL_CELL_SIZE: f32 = 160.0;
    const POINT_SELECT_SEARCH_RADIUS: f32 = 320.0;
    const BOUNDS_SELECT_PADDING: f32 = 192.0;

    pub fn spawn(&mut self, faction: FactionId, position: Vec2) -> UnitId {
        let id = UnitId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.units.push(RtsUnit::new(id, faction, position));
        self.spatial_dirty.set(true);
        id
    }

    pub fn units(&self) -> &[RtsUnit] {
        &self.units
    }

    pub fn units_mut(&mut self) -> &mut [RtsUnit] {
        self.spatial_dirty.set(true);
        &mut self.units
    }

    pub fn unit(&self, id: UnitId) -> Option<&RtsUnit> {
        self.units.iter().find(|unit| unit.id == id)
    }

    pub fn unit_mut(&mut self, id: UnitId) -> Option<&mut RtsUnit> {
        if let Some(unit) = self.units.iter_mut().find(|unit| unit.id == id) {
            self.spatial_dirty.set(true);
            return Some(unit);
        }
        None
    }

    fn rebuild_spatial_index_if_dirty(&self) {
        if !self.spatial_dirty.get() {
            return;
        }
        let mut index = self.spatial_index.borrow_mut();
        index.build(&self.units);
        self.spatial_dirty.set(false);
    }

    /// Finds the most critically damaged living ally in range.
    ///
    /// Candidates are ranked by remaining health percentage, then by stable
    /// unit ID so support behavior remains deterministic across platforms.
    pub fn most_damaged_ally_in_range(
        &self,
        origin: UnitId,
        faction: FactionId,
        range: f32,
    ) -> Option<UnitId> {
        let origin_position = self.unit(origin)?.position;
        let range = range.max(0.0);
        let range_sq = range * range;
        self.rebuild_spatial_index_if_dirty();
        let candidates = {
            let index = self.spatial_index.borrow();
            index.query_cell_ids(origin_position, range)
        };

        candidates
            .into_iter()
            .filter_map(|id| {
                let unit = self.unit(id)?;
                ((unit.id != origin)
                    && unit.faction == faction
                    && unit.alive()
                    && unit.max_health > 0.0
                    && unit.health < unit.max_health
                    && unit.position.distance_squared(origin_position) <= range_sq)
                .then_some(unit.id)
            })
            .min_by(|left, right| {
                let left_unit = self.unit(*left).unwrap();
                let right_unit = self.unit(*right).unwrap();
                (left_unit.health / left_unit.max_health)
                    .total_cmp(&(right_unit.health / right_unit.max_health))
                    .then_with(|| left_unit.id.0.cmp(&right_unit.id.0))
            })
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Units currently assigned to `slot` (regardless of whether they're
    /// still alive/friendly — see [`RtsWorld::recall_control_group`] for the
    /// filtered version used to actually select them).
    pub fn control_group(&self, slot: usize) -> &[UnitId] {
        self.control_groups
            .get(slot)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn assign_control_group(&mut self, slot: usize) -> bool {
        let Some(group) = self.control_groups.get_mut(slot) else {
            return false;
        };
        *group = self.selection.ids.clone();
        true
    }

    pub fn recall_control_group(&mut self, slot: usize, faction: FactionId) -> bool {
        let Some(group) = self.control_groups.get(slot) else {
            return false;
        };
        self.selection.ids = group
            .iter()
            .copied()
            .filter(|id| {
                self.units
                    .iter()
                    .any(|unit| unit.id == *id && unit.faction == faction && unit.alive())
            })
            .collect();
        !self.selection.ids.is_empty()
    }

    pub fn select_point(&mut self, point: Vec2, faction: FactionId, additive: bool) {
        if !additive {
            self.selection.clear();
        }
        self.rebuild_spatial_index_if_dirty();
        let mut candidates = {
            let index = self.spatial_index.borrow();
            index.query_cell_ids(point, Self::POINT_SELECT_SEARCH_RADIUS)
        };
        if candidates.is_empty() {
            let index = self.spatial_index.borrow();
            candidates = index.query_cell_ids(point, f32::INFINITY);
        }
        let selected = candidates
            .into_iter()
            .filter_map(|id| self.unit(id))
            .filter(|unit| unit.alive() && unit.faction == faction)
            .filter_map(|unit| {
                let radius = unit.radius.max(0.0) * 1.35;
                let distance_sq = unit.position.distance_squared(point);
                let radius_sq = radius * radius;
                (distance_sq <= radius_sq).then_some((unit.id, distance_sq))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.0.cmp(&b.0.0)))
            .map(|(id, _)| id);
        if let Some(id) = selected {
            self.add_selected(id);
        }
    }

    /// Adds a caller-selected set of living units from one faction to the
    /// current selection. Games use this for semantic selection gestures such
    /// as Ctrl-clicking a unit type; the engine still validates ownership and
    /// liveness so stale presentation IDs cannot leak into orders.
    ///
    /// Returning the number of accepted IDs gives a UI a cheap way to report
    /// an empty result without inspecting the private selection buffer.
    pub fn select_ids(&mut self, ids: &[UnitId], faction: FactionId, additive: bool) -> usize {
        if !additive {
            self.selection.clear();
        }
        let mut accepted = 0;
        for id in ids.iter().copied() {
            let valid = self
                .unit(id)
                .is_some_and(|unit| unit.alive() && unit.faction == faction);
            if valid && !self.selection.contains(id) {
                self.add_selected(id);
                accepted += 1;
            }
        }
        accepted
    }

    pub fn select_bounds(&mut self, bounds: Aabb, faction: FactionId, additive: bool) {
        if !additive {
            self.selection.clear();
        }
        self.rebuild_spatial_index_if_dirty();
        let padded = Aabb::new(
            bounds.min - Vec2::splat(Self::BOUNDS_SELECT_PADDING),
            bounds.max + Vec2::splat(Self::BOUNDS_SELECT_PADDING),
        );
        let candidates = {
            let index = self.spatial_index.borrow();
            index.query_aabb_ids(padded)
        };
        let ids: Vec<UnitId> = candidates
            .into_iter()
            .filter_map(|id| self.unit(id))
            .filter(|unit| {
                unit.alive()
                    && unit.faction == faction
                    // Marquee selection is footprint-aware: a unit whose
                    // visible body overlaps the drag box is selectable even
                    // when its center is just outside the box.
                    && Aabb::from_center_size(
                        unit.position,
                        Vec2::splat(unit.radius.max(0.0) * 2.0),
                    )
                    .intersects(bounds)
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
        for (id, target) in self.formation_destinations(destination, spacing) {
            if let Some(unit) = self.unit_mut(id) {
                unit.order = UnitOrder::Move(target);
                unit.queued_orders.clear();
            }
        }
    }

    pub fn queue_move(&mut self, destination: Vec2, spacing: f32) {
        for (id, target) in self.formation_destinations(destination, spacing) {
            if let Some(unit) = self.unit_mut(id) {
                let order = UnitOrder::Move(target);
                if matches!(unit.order, UnitOrder::Idle) {
                    unit.order = order;
                } else {
                    unit.queued_orders.push_back(order);
                }
            }
        }
    }

    pub fn issue_attack_move(&mut self, destination: Vec2, append: bool) {
        self.issue_attack_move_with_spacing(destination, self.default_formation_spacing(), append);
    }

    /// Issues an attack-move while preserving squad cohesion with deterministic
    /// formation slots. Use this when a game has authored spacing; the simpler
    /// [`Self::issue_attack_move`] derives spacing from the selected units.
    pub fn issue_attack_move_with_spacing(
        &mut self,
        destination: Vec2,
        spacing: f32,
        append: bool,
    ) {
        for (id, target) in self.formation_destinations(destination, spacing) {
            if let Some(unit) = self.unit_mut(id) {
                if append && !matches!(unit.order, UnitOrder::Idle) {
                    unit.queued_orders.push_back(UnitOrder::AttackMove(target));
                } else {
                    unit.order = UnitOrder::AttackMove(target);
                    unit.queued_orders.clear();
                }
            }
        }
    }

    fn default_formation_spacing(&self) -> f32 {
        let max_radius = self
            .selection
            .ids
            .iter()
            .filter_map(|id| self.unit(*id).map(|unit| unit.radius.max(0.0)))
            .fold(0.0, f32::max);
        (max_radius * 2.5).max(1.0)
    }

    fn formation_destinations(&self, destination: Vec2, spacing: f32) -> Vec<(UnitId, Vec2)> {
        let ids = &self.selection.ids;
        let width = (ids.len() as f32).sqrt().ceil().max(1.0) as usize;
        let rows = ids.len().div_ceil(width);
        let spacing = spacing.max(0.0);
        ids.iter()
            .copied()
            .enumerate()
            .map(|(index, id)| {
                let column = index % width;
                let row = index / width;
                let centered = Vec2::new(
                    column as f32 - (width.saturating_sub(1)) as f32 * 0.5,
                    row as f32 - rows.saturating_sub(1) as f32 * 0.5,
                );
                (id, destination + centered * spacing)
            })
            .collect()
    }

    pub fn issue_patrol(&mut self, destination: Vec2, append: bool) {
        let ids = self.selection.ids.clone();
        for id in ids {
            let Some(unit) = self.unit(id) else {
                continue;
            };
            let start = unit.position;
            if let Some(unit) = self.unit_mut(id) {
                let order = UnitOrder::Patrol(start, destination);
                if append && !matches!(unit.order, UnitOrder::Idle) {
                    unit.queued_orders.push_back(order);
                } else {
                    unit.order = order;
                    unit.queued_orders.clear();
                }
            }
        }
    }

    pub fn issue_follow(&mut self, target: UnitId, append: bool) {
        let ids = self.selection.ids.clone();
        for id in ids {
            if id == target {
                continue;
            }
            if let Some(unit) = self.unit_mut(id) {
                if append && !matches!(unit.order, UnitOrder::Idle) {
                    unit.queued_orders.push_back(UnitOrder::Follow(target));
                } else {
                    unit.order = UnitOrder::Follow(target);
                    unit.queued_orders.clear();
                }
            }
        }
    }

    pub fn start_next_queued_order(&mut self, id: UnitId) -> bool {
        let Some(unit) = self.unit_mut(id) else {
            return false;
        };
        let Some(order) = unit.queued_orders.pop_front() else {
            return false;
        };
        unit.order = order;
        true
    }

    /// Issues an explicit attack order to every selected unit.
    ///
    /// With `append` enabled this behaves like a shift-click attack command:
    /// units that are already busy keep their current order and attack the
    /// target after their existing queue. Idle units begin attacking
    /// immediately. A non-appended command replaces both the active order and
    /// any queued work, matching the rest of the imperative `issue_*` APIs.
    pub fn issue_attack_order(&mut self, target: UnitId, append: bool) {
        let ids = self.selection.ids.clone();
        for id in ids {
            if let Some(unit) = self.unit_mut(id) {
                let order = UnitOrder::Attack(target);
                if append && !matches!(unit.order, UnitOrder::Idle) {
                    unit.queued_orders.push_back(order);
                } else {
                    unit.order = order;
                    unit.queued_orders.clear();
                }
            }
        }
    }

    /// Replaces the selected units' current work with an explicit attack.
    /// Kept as the source-compatible shorthand for the pre-append API.
    pub fn issue_attack(&mut self, target: UnitId) {
        self.issue_attack_order(target, false);
    }

    pub fn issue_hold(&mut self) {
        let ids = self.selection.ids.clone();
        for id in ids {
            if let Some(unit) = self.unit_mut(id) {
                unit.order = UnitOrder::Hold;
                unit.velocity = Vec2::ZERO;
                unit.queued_orders.clear();
            }
        }
    }

    /// Cancels all work for the currently selected living units.
    ///
    /// Unlike [`Self::issue_hold`], this is a true stop command: it returns
    /// units to [`UnitOrder::Idle`], drops every queued order, and clears any
    /// residual velocity in the same deterministic pass. Selection can outlive
    /// a unit (for example, when it dies between input frames), so stale or
    /// dead IDs are ignored rather than mutating an invalid combatant.
    pub fn issue_stop(&mut self) {
        let ids = self.selection.ids.clone();
        for id in ids {
            let Some(unit) = self.unit_mut(id) else {
                continue;
            };
            if !unit.alive() {
                continue;
            }
            unit.order = UnitOrder::Idle;
            unit.queued_orders.clear();
            unit.velocity = Vec2::ZERO;
        }
    }

    pub fn update(&mut self, dt: f32) {
        let dt = dt.max(0.0);
        let positions: Vec<(UnitId, Vec2, bool)> = self
            .units
            .iter()
            .map(|unit| (unit.id, unit.position, unit.alive()))
            .collect();
        let mut moved = false;
        for unit in &mut self.units {
            let previous_position = unit.position;
            if !unit.alive() {
                unit.velocity = Vec2::ZERO;
                continue;
            }
            let target = match unit.order {
                UnitOrder::Move(target)
                | UnitOrder::AttackMove(target)
                | UnitOrder::Interact(target) => Some(target),
                UnitOrder::Attack(id) => positions
                    .iter()
                    .find(|(candidate, _, alive)| *candidate == id && *alive)
                    .map(|(_, position, _)| *position),
                UnitOrder::Follow(id) => positions
                    .iter()
                    .find(|(candidate, _, alive)| *candidate == id && *alive)
                    .map(|(_, position, _)| *position),
                UnitOrder::Patrol(first, _) => Some(first),
                UnitOrder::Idle | UnitOrder::Hold => None,
            };
            // Orders that reference a destroyed or removed unit must not
            // strand a worker in a permanently busy state. Advance one
            // queued order immediately; the next update will validate that
            // order too if it is another stale target reference.
            if matches!(unit.order, UnitOrder::Attack(_) | UnitOrder::Follow(_)) && target.is_none()
            {
                unit.velocity = Vec2::ZERO;
                unit.order = unit.queued_orders.pop_front().unwrap_or(UnitOrder::Idle);
                continue;
            }
            let Some(target) = target else {
                unit.velocity = Vec2::ZERO;
                continue;
            };
            let offset = target - unit.position;
            let distance = offset.length();
            let arrival_radius = unit.radius.max(0.0) * 0.35;
            let engagement_range = match unit.order {
                UnitOrder::Attack(_) => unit.engagement_range.max(0.0),
                _ => 0.0,
            };
            // Attack orders have a distinct firing line. Once a unit is
            // inside that envelope it must hold position and keep its order;
            // the combat resolver can then apply damage without the movement
            // integrator pushing the unit through the target.
            if engagement_range > arrival_radius && distance <= engagement_range {
                unit.velocity = Vec2::ZERO;
                continue;
            }
            // Clamp the integration step to the destination. Without this,
            // a hitch or a headless simulation tick can overshoot a waypoint
            // and make the unit oscillate around it forever.
            let step = unit.speed.max(0.0) * dt;
            let stopping_distance = engagement_range.max(arrival_radius);
            if distance <= arrival_radius || distance <= step + stopping_distance {
                if engagement_range > arrival_radius && distance > f32::EPSILON {
                    // Place the unit exactly on the near edge of its firing
                    // envelope. The order remains Attack, so a later target
                    // death or retarget can advance naturally.
                    unit.position = target - offset / distance * engagement_range;
                } else {
                    unit.position = target;
                }
                unit.velocity = Vec2::ZERO;
                match unit.order {
                    UnitOrder::Move(_) | UnitOrder::AttackMove(_) | UnitOrder::Interact(_)
                        if engagement_range <= arrival_radius =>
                    {
                        unit.order = unit.queued_orders.pop_front().unwrap_or(UnitOrder::Idle);
                    }
                    UnitOrder::Patrol(first, second) => {
                        unit.order = UnitOrder::Patrol(second, first);
                    }
                    _ => {}
                }
            } else {
                unit.velocity = offset / distance * unit.speed.max(0.0);
                unit.position += unit.velocity * dt;
            }
            if unit.position != previous_position {
                moved = true;
            }
        }
        if moved {
            self.spatial_dirty.set(true);
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
    version: u64,
}

impl NavGrid {
    pub fn new(width: usize, height: usize, origin: Vec2, cell_size: f32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            origin,
            cell_size: cell_size.max(1.0),
            blocked: vec![false; width.max(1) * height.max(1)],
            version: 0,
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
            if self.blocked[index] == blocked {
                return;
            }
            self.blocked[index] = blocked;
            self.version = self.version.saturating_add(1);
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_blocked_at(&self, world: Vec2) -> bool {
        self.index(self.world_to_cell(world))
            .map(|index| self.blocked[index])
            .unwrap_or(false)
    }

    /// Samples a straight line between two world points and reports whether
    /// any sampled point falls in a blocked cell. Used to decide whether a
    /// direct approach needs to fall back to `find_path`.
    pub fn segment_blocked(&self, start: Vec2, end: Vec2) -> bool {
        let distance = start.distance(end);
        if distance <= f32::EPSILON {
            return self.is_blocked_at(start);
        }
        let steps = (distance / (self.cell_size * 0.5)).ceil().max(1.0) as u32;
        (0..=steps).any(|step| self.is_blocked_at(start.lerp(end, step as f32 / steps as f32)))
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
        let cell_waypoints: Vec<Vec2> = cells
            .into_iter()
            .skip(1)
            .map(|cell| self.cell_center(cell))
            .collect();

        // Collapse the raw cell-center route into the longest visible segments.
        // This keeps BFS's deterministic ordering while avoiding a stop-start
        // movement cadence on every cardinal cell.
        let mut path = Vec::with_capacity(cell_waypoints.len() + 1);
        let mut anchor = start_world;
        let mut next = 0;
        while next < cell_waypoints.len() {
            let mut furthest = next;
            for (candidate, waypoint) in cell_waypoints.iter().enumerate().skip(next) {
                if !self.segment_blocked(anchor, *waypoint) {
                    furthest = candidate;
                }
            }
            let waypoint = cell_waypoints[furthest];
            path.push(waypoint);
            anchor = waypoint;
            next = furthest + 1;
        }

        // Cell centers are only an intermediate navigation representation. The
        // caller's world-space destination is always the final target when its
        // final segment is clear (which it is for an unblocked goal cell).
        if anchor.distance(goal_world) > f32::EPSILON && !self.segment_blocked(anchor, goal_world) {
            path.push(goal_world);
        }
        path
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

    #[test]
    fn placement_rules_report_power_bounds_and_obstructions() {
        let rules = PlacementRules {
            build_area: Aabb::from_center_size(Vec2::ZERO, Vec2::splat(1000.0)),
            power_sources: vec![Vec2::ZERO],
            obstructions: vec![(Vec2::new(100.0, 0.0), 40.0)],
            max_power_distance: 300.0,
        };
        assert_eq!(rules.validate(Vec2::new(0.0, 200.0), 30.0), Ok(()));
        assert_eq!(
            rules.validate(Vec2::new(420.0, 0.0), 30.0),
            Err(PlacementError::TooFarFromPower)
        );
        assert_eq!(
            rules.validate(Vec2::new(100.0, 0.0), 30.0),
            Err(PlacementError::Obstructed)
        );
        assert_eq!(
            rules.validate(Vec2::new(-490.0, 0.0), 30.0),
            Err(PlacementError::OutsideBuildArea)
        );
    }

    #[test]
    fn minimap_transform_round_trips_world_positions() {
        let transform = MinimapTransform {
            world: Aabb::new(Vec2::new(-1000.0, -500.0), Vec2::new(1000.0, 500.0)),
            panel: Aabb::new(Vec2::new(20.0, 30.0), Vec2::new(220.0, 130.0)),
        };
        let world = Vec2::new(250.0, -125.0);
        let panel = transform.world_to_panel(world);
        assert!(transform.panel_to_world(panel).unwrap().distance(world) < 0.01);
        assert_eq!(transform.panel_to_world(Vec2::ZERO), None);
    }

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
    fn marquee_selection_includes_footprint_overlap_without_nearby_units() {
        let mut world = RtsWorld::default();
        let edge = world.spawn(PLAYER, Vec2::new(20.0, 0.0));
        world.unit_mut(edge).unwrap().radius = 12.0;
        let outside = world.spawn(PLAYER, Vec2::new(26.0, 0.0));
        world.unit_mut(outside).unwrap().radius = 4.0;

        world.select_bounds(
            Aabb::from_center_size(Vec2::ZERO, Vec2::splat(20.0)),
            PLAYER,
            false,
        );

        assert_eq!(world.selection().ids(), &[edge]);
    }

    #[test]
    fn semantic_select_ids_filters_stale_or_hostile_units() {
        let mut world = RtsWorld::default();
        let first = world.spawn(PLAYER, Vec2::ZERO);
        let second = world.spawn(PLAYER, Vec2::X);
        let dead = world.spawn(PLAYER, Vec2::Y);
        let hostile = world.spawn(FactionId(2), Vec2::new(2.0, 0.0));
        world.unit_mut(dead).unwrap().health = 0.0;

        assert_eq!(
            world.select_ids(&[first, second, dead, hostile, first], PLAYER, false),
            2
        );
        assert_eq!(world.selection().ids(), &[first, second]);

        let third = world.spawn(PLAYER, Vec2::new(3.0, 0.0));
        assert_eq!(world.select_ids(&[second, third], PLAYER, true), 1);
        assert_eq!(world.selection().ids(), &[first, second, third]);
    }

    #[test]
    fn attack_move_keeps_selected_units_in_a_deterministic_formation() {
        let mut world = RtsWorld::default();
        world.spawn(PLAYER, Vec2::new(-10.0, 0.0));
        world.spawn(PLAYER, Vec2::new(10.0, 0.0));
        world.select_bounds(
            Aabb::from_center_size(Vec2::ZERO, Vec2::splat(100.0)),
            PLAYER,
            false,
        );

        let destination = Vec2::new(200.0, 100.0);
        world.issue_attack_move_with_spacing(destination, 40.0, false);
        let orders: Vec<UnitOrder> = world.units().iter().map(|unit| unit.order).collect();
        assert_eq!(
            orders,
            [
                UnitOrder::AttackMove(Vec2::new(180.0, 100.0)),
                UnitOrder::AttackMove(Vec2::new(220.0, 100.0)),
            ]
        );

        // The legacy command derives safe spacing from unit radii, so it also
        // keeps a squad apart without requiring a game-specific constant.
        world.issue_attack_move(destination, false);
        let defaults: Vec<Vec2> = world
            .units()
            .iter()
            .filter_map(|unit| match unit.order {
                UnitOrder::AttackMove(target) => Some(target),
                _ => None,
            })
            .collect();
        assert_eq!(defaults, [Vec2::new(165.0, 100.0), Vec2::new(235.0, 100.0)]);
    }

    #[test]
    fn navigation_routes_around_blocked_cells() {
        let mut grid = NavGrid::new(5, 3, Vec2::ZERO, 10.0);
        grid.set_blocked(IVec2::new(2, 1), true);
        let goal = Vec2::new(45.0, 15.0);
        let path = grid.find_path(Vec2::new(5.0, 15.0), goal);
        assert!(!path.is_empty());
        assert_eq!(path.last().copied(), Some(goal));
        assert!(path
            .iter()
            .all(|point| grid.world_to_cell(*point) != IVec2::new(2, 1)));
    }

    #[test]
    fn navigation_smooths_clear_cells_without_crossing_blockers() {
        let mut grid = NavGrid::new(7, 5, Vec2::ZERO, 10.0);
        grid.set_blocked(IVec2::new(3, 2), true);
        let start = Vec2::new(5.0, 25.0);
        let goal = Vec2::new(65.0, 25.0);
        let path = grid.find_path(start, goal);

        assert_eq!(path.last().copied(), Some(goal));
        assert!(path.len() <= 3, "smoothed path was {path:?}");
        let mut anchor = start;
        for waypoint in path {
            assert!(!grid.segment_blocked(anchor, waypoint));
            anchor = waypoint;
        }
    }

    #[test]
    fn segment_blocked_detects_obstacles_between_endpoints() {
        let mut grid = NavGrid::new(5, 3, Vec2::ZERO, 10.0);
        assert!(!grid.segment_blocked(Vec2::new(5.0, 15.0), Vec2::new(45.0, 15.0)));
        grid.set_blocked(IVec2::new(2, 1), true);
        assert!(grid.segment_blocked(Vec2::new(5.0, 15.0), Vec2::new(45.0, 15.0)));
        assert!(!grid.segment_blocked(Vec2::new(5.0, 5.0), Vec2::new(45.0, 5.0)));
    }

    #[test]
    fn fog_transitions_from_visible_to_explored() {
        let mut fog = FogOfWar::new(8, 8, Vec2::ZERO, 10.0);
        fog.reveal(Vec2::new(25.0, 25.0), 12.0);
        assert_eq!(fog.state_at(Vec2::new(25.0, 25.0)), FogState::Visible);
        fog.begin_frame();
        assert_eq!(fog.state_at(Vec2::new(25.0, 25.0)), FogState::Explored);
    }

    #[test]
    fn production_spends_once_and_completes_in_queue_order() {
        let mut resources = ResourceBank::new(100);
        let mut queue = ProductionQueue::new(2);
        let scout = ProductionRecipe::new(ProductId(7), 40, 1_000);
        let warden = ProductionRecipe::new(ProductId(9), 60, 2_000);
        assert_eq!(queue.enqueue(scout, &mut resources), Ok(()));
        assert_eq!(queue.enqueue(warden, &mut resources), Ok(()));
        assert_eq!(resources.amount(), 0);
        assert_eq!(queue.update(1.5), [ProductId(7)]);
        assert!((queue.items()[0].progress() - 0.25).abs() < 0.001);
        assert_eq!(queue.update(1.5), [ProductId(9)]);
    }

    #[test]
    fn supply_aware_production_admission_is_atomic() {
        let recipe = ProductionRecipe::new(ProductId(12), 40, 1_000);
        let mut queue = ProductionQueue::new(1);
        let mut resources = ResourceBank::new(50);
        let mut supply = SupplyLedger::new(2);

        assert_eq!(
            queue.enqueue_with_supply(recipe, &mut resources, &mut supply, 2),
            Ok(())
        );
        assert_eq!(resources.amount(), 10);
        assert_eq!(supply.used(), 2);

        // Capacity is checked before either wallet is touched.
        assert_eq!(
            queue.enqueue_with_supply(recipe, &mut resources, &mut supply, 1),
            Err(SupplyQueueError::Full)
        );
        assert_eq!(resources.amount(), 10);
        assert_eq!(supply.used(), 2);

        let mut resource_blocked = ProductionQueue::new(1);
        let mut poor_resources = ResourceBank::new(39);
        let mut open_supply = SupplyLedger::new(2);
        assert_eq!(
            resource_blocked.enqueue_with_supply(recipe, &mut poor_resources, &mut open_supply, 1,),
            Err(SupplyQueueError::InsufficientResources)
        );
        assert_eq!(poor_resources.amount(), 39);
        assert_eq!(open_supply.used(), 0);

        let mut supply_blocked = ProductionQueue::new(1);
        let mut enough_resources = ResourceBank::new(50);
        let mut capped_supply = SupplyLedger::new(1);
        assert_eq!(
            supply_blocked.enqueue_with_supply(
                recipe,
                &mut enough_resources,
                &mut capped_supply,
                2,
            ),
            Err(SupplyQueueError::InsufficientSupply)
        );
        assert_eq!(enough_resources.amount(), 50);
        assert_eq!(capped_supply.used(), 0);
    }

    #[test]
    fn production_cancellation_refunds_metadata_and_releases_supply() {
        let first = ProductionRecipe::new(ProductId(30), 40, 1_000);
        let second = ProductionRecipe::new(ProductId(31), 60, 2_000);
        let mut queue = ProductionQueue::new(2);
        let mut resources = ResourceBank::new(100);
        let mut supply = SupplyLedger::new(3);
        queue.enqueue(first, &mut resources).unwrap();
        queue
            .enqueue_with_supply(second, &mut resources, &mut supply, 2)
            .unwrap();
        assert_eq!(resources.amount(), 0);
        assert_eq!(supply.used(), 2);

        // A supply-backed job cannot be cancelled through the legacy API, so
        // callers cannot accidentally leave the ledger permanently blocked.
        assert_eq!(
            queue.cancel(1, &mut resources, 100),
            Err(ProductionCancelError::SupplyLedgerRequired)
        );
        assert_eq!(resources.amount(), 0);
        assert_eq!(supply.used(), 2);

        let receipt = queue.cancel(0, &mut resources, 75).unwrap();
        assert_eq!(
            receipt,
            ProductionCancelReceipt {
                product: ProductId(30),
                refunded_resources: 30,
                released_supply: 0,
            }
        );
        assert_eq!(resources.amount(), 30);
        assert_eq!(supply.used(), 2);
        assert_eq!(queue.items()[0].product, ProductId(31));

        let receipt = queue
            .cancel_with_supply(0, &mut resources, &mut supply, 150)
            .unwrap();
        assert_eq!(receipt.refunded_resources, 60);
        assert_eq!(receipt.released_supply, 2);
        assert_eq!(resources.amount(), 90);
        assert_eq!(supply.used(), 0);
        assert!(queue.items().is_empty());
        assert_eq!(
            queue.cancel(0, &mut resources, 100),
            Err(ProductionCancelError::InvalidIndex)
        );
        assert_eq!(resources.amount(), 90);
    }

    #[test]
    fn build_queue_carries_elapsed_time_across_structure_jobs() {
        let first = BuildId(10);
        let second = BuildId(11);
        let mut queue = BuildQueue::new(2);
        queue
            .enqueue(BuildRecipe::new(first, 2.0))
            .expect("first build fits");
        queue
            .enqueue(BuildRecipe::new(second, 3.0))
            .expect("second build fits");
        assert_eq!(queue.front().unwrap().progress(), 0.0);
        assert_eq!(queue.update(2.5), [first]);
        assert_eq!(queue.front().unwrap().build, second);
        assert!((queue.front().unwrap().progress() - (0.5 / 3.0)).abs() < 0.001);
        assert_eq!(queue.update(0.5), []);
        assert_eq!(queue.update(2.5), [second]);
        assert!(queue.is_empty());
        assert_eq!(queue.enqueue(BuildRecipe::new(BuildId(12), 1.0)), Ok(()));
    }

    #[test]
    fn build_queue_rejects_jobs_above_capacity() {
        let mut queue = BuildQueue::new(1);
        queue
            .enqueue(BuildRecipe::new(BuildId(20), 1.0))
            .expect("first build fits");
        assert_eq!(
            queue.enqueue(BuildRecipe::new(BuildId(21), 1.0)),
            Err(BuildQueueError::Full)
        );
    }

    #[test]
    fn resource_supply_and_tech_contracts_are_deterministic() {
        let mut resources = ResourceSet::new(100, 2);
        assert!(resources.spend(ResourceCost::new(40, 1)));
        assert_eq!(resources, ResourceSet::new(60, 1));
        assert!(!resources.spend(ResourceCost::new(61, 0)));

        let mut supply = SupplyLedger::new(3);
        assert!(supply.try_add(2));
        assert!(!supply.try_add(2));
        supply.release(1);
        assert_eq!(supply.available(), 2);

        let base = TechId(1);
        let advanced = TechId(2);
        let mut tech = TechGraph::default();
        tech.define(base, Vec::new());
        tech.define(advanced, vec![base]);
        assert!(tech.can_unlock(base));
        assert!(!tech.can_unlock(advanced));
        assert!(tech.unlock(base));
        assert!(tech.unlock(advanced));
    }

    #[test]
    fn tactical_damage_and_cover_preserve_class_counters() {
        assert_eq!(DamageType::Concussive.multiplier(ArmorClass::Large), 0.25);
        assert_eq!(DamageType::Explosive.multiplier(ArmorClass::Small), 0.5);
        let cover = TerrainZone::new(
            Aabb::from_center_size(Vec2::ZERO, Vec2::splat(100.0)),
            1,
            0.2,
        );
        assert!((cover.damage_multiplier(0) - 0.56).abs() < 0.001);
    }

    #[test]
    fn terrain_classification_is_stable_for_overlay_contracts() {
        let bounds = Aabb::from_center_size(Vec2::ZERO, Vec2::splat(100.0));
        assert_eq!(
            TerrainZone::new(bounds, 0, 0.0).classification(),
            TerrainClass::Open
        );
        assert_eq!(
            TerrainZone::new(bounds, 0, TERRAIN_COVER_THRESHOLD).classification(),
            TerrainClass::Covered
        );
        assert_eq!(
            TerrainZone::new(bounds, 1, TERRAIN_COVER_THRESHOLD - 0.01).classification(),
            TerrainClass::HighGround
        );
        assert_eq!(
            TerrainZone::new(bounds, 1, TERRAIN_MAX_COVER + 1.0).classification(),
            TerrainClass::FortifiedHighGround
        );
        assert!(TerrainClass::FortifiedHighGround.has_cover());
        assert!(TerrainClass::FortifiedHighGround.is_high_ground());

        // Editor-authored malformed values stay finite and render as open
        // terrain until the mission validator reports the authoring error.
        let malformed = TerrainZone::new(bounds, 0, f32::NAN);
        assert_eq!(malformed.normalized_cover(), 0.0);
        assert_eq!(malformed.classification(), TerrainClass::Open);
        assert_eq!(malformed.damage_multiplier(0), 1.0);
    }

    #[test]
    fn terrain_resolver_prefers_strategic_strength_and_keeps_authored_ties() {
        let bounds = Aabb::from_center_size(Vec2::ZERO, Vec2::splat(100.0));
        let zones = [
            TerrainZone::new(bounds, 0, 0.0),
            TerrainZone::new(bounds, 0, TERRAIN_MAX_COVER),
            TerrainZone::new(bounds, 1, 0.0),
            TerrainZone::new(bounds, 1, TERRAIN_COVER_THRESHOLD),
        ];

        let (index, zone) = TerrainZone::resolve_at(Vec2::ZERO, &zones).unwrap();
        assert_eq!(index, 3);
        assert_eq!(zone.classification(), TerrainClass::FortifiedHighGround);
        assert_eq!(TerrainZone::resolve_at(Vec2::new(101.0, 0.0), &zones), None);

        let tied = [
            TerrainZone::new(bounds, 1, TERRAIN_COVER_THRESHOLD),
            TerrainZone::new(bounds, 1, TERRAIN_COVER_THRESHOLD),
        ];
        assert_eq!(TerrainZone::resolve_at(Vec2::ZERO, &tied).unwrap().0, 0);
    }

    #[test]
    fn terrain_readout_is_compact_and_finite_for_hud_consumers() {
        let bounds = Aabb::from_center_size(Vec2::ZERO, Vec2::splat(100.0));
        let zones = [
            TerrainZone::new(bounds, 0, 0.0),
            TerrainZone::new(bounds, 0, TERRAIN_COVER_THRESHOLD),
            TerrainZone::new(bounds, 2, TERRAIN_MAX_COVER),
        ];

        let open = zones[0].readout();
        assert_eq!(
            open,
            TerrainReadout {
                class: TerrainClass::Open,
                elevation: 0,
                cover_percent: 0,
            }
        );
        let covered = zones[1].readout();
        assert_eq!(covered.class, TerrainClass::Covered);
        assert_eq!(covered.cover_percent, 20);
        assert!(covered.has_cover());

        let (index, high) = TerrainZone::resolve_readout_at(Vec2::ZERO, &zones).unwrap();
        assert_eq!(index, 2);
        assert_eq!(high.class, TerrainClass::FortifiedHighGround);
        assert_eq!(high.elevation, 2);
        assert_eq!(high.cover_percent, 30);
        assert!(high.is_high_ground());

        let malformed = TerrainZone::new(bounds, -2, f32::INFINITY).readout();
        assert_eq!(malformed.class, TerrainClass::Open);
        assert_eq!(malformed.elevation, -2);
        assert_eq!(malformed.cover_percent, 0);
    }

    #[test]
    fn queued_orders_start_after_waypoint_arrival() {
        let mut world = RtsWorld::default();
        let id = world.spawn(PLAYER, Vec2::ZERO);
        world.select_point(Vec2::ZERO, PLAYER, false);
        world.issue_move(Vec2::new(50.0, 0.0), 0.0);
        world.queue_move(Vec2::new(100.0, 0.0), 0.0);
        world.unit_mut(id).unwrap().position = Vec2::new(50.0, 0.0);
        world.update(0.0);
        assert!(matches!(
            world.unit(id).unwrap().order,
            UnitOrder::Move(destination) if destination == Vec2::new(100.0, 0.0)
        ));
        assert!(world.unit(id).unwrap().queued_orders.is_empty());
    }

    #[test]
    fn appended_attack_orders_preserve_current_work_and_fifo_order() {
        let mut world = RtsWorld::default();
        let first = world.spawn(PLAYER, Vec2::ZERO);
        let second = world.spawn(PLAYER, Vec2::new(20.0, 0.0));
        let first_target = world.spawn(FactionId(2), Vec2::new(100.0, 0.0));
        let second_target = world.spawn(FactionId(2), Vec2::new(120.0, 0.0));

        world.select_bounds(
            Aabb::from_center_size(Vec2::new(10.0, 0.0), Vec2::splat(80.0)),
            PLAYER,
            false,
        );
        world.issue_move(Vec2::new(60.0, 0.0), 0.0);
        world.issue_attack_order(first_target, true);
        world.issue_attack_order(second_target, true);

        for id in [first, second] {
            let unit = world.unit(id).unwrap();
            assert_eq!(unit.order, UnitOrder::Move(Vec2::new(60.0, 0.0)));
            assert_eq!(
                unit.queued_orders.iter().copied().collect::<Vec<_>>(),
                [
                    UnitOrder::Attack(first_target),
                    UnitOrder::Attack(second_target),
                ]
            );
        }

        // A non-appended command is an immediate replacement and clears the
        // queued attack chain for every selected unit.
        world.issue_attack(second_target);
        for id in [first, second] {
            let unit = world.unit(id).unwrap();
            assert_eq!(unit.order, UnitOrder::Attack(second_target));
            assert!(unit.queued_orders.is_empty());
        }
    }

    #[test]
    fn stop_command_clears_selected_orders_queues_and_velocity() {
        let mut world = RtsWorld::default();
        let first = world.spawn(PLAYER, Vec2::ZERO);
        let second = world.spawn(PLAYER, Vec2::new(20.0, 0.0));
        let target = world.spawn(FactionId(2), Vec2::new(200.0, 0.0));
        assert_eq!(world.select_ids(&[first, second], PLAYER, false), 2);

        world.unit_mut(first).unwrap().order = UnitOrder::Move(Vec2::new(80.0, 0.0));
        world.unit_mut(first).unwrap().velocity = Vec2::new(12.0, -4.0);
        world.unit_mut(first).unwrap().queued_orders.extend([
            UnitOrder::Attack(target),
            UnitOrder::Move(Vec2::new(160.0, 0.0)),
        ]);
        world.unit_mut(second).unwrap().order = UnitOrder::AttackMove(Vec2::X * 90.0);
        world.unit_mut(second).unwrap().velocity = Vec2::new(-8.0, 3.0);
        world
            .unit_mut(second)
            .unwrap()
            .queued_orders
            .push_back(UnitOrder::Follow(first));

        world.issue_stop();

        for id in [first, second] {
            let unit = world.unit(id).unwrap();
            assert_eq!(unit.order, UnitOrder::Idle);
            assert!(unit.queued_orders.is_empty());
            assert_eq!(unit.velocity, Vec2::ZERO);
        }
    }

    #[test]
    fn stop_command_excludes_stale_dead_and_hostile_units() {
        let mut world = RtsWorld::default();
        let living = world.spawn(PLAYER, Vec2::ZERO);
        let stale = world.spawn(PLAYER, Vec2::new(20.0, 0.0));
        let hostile = world.spawn(FactionId(2), Vec2::new(40.0, 0.0));

        // The hostile ID is rejected at selection time. The second friendly
        // unit is selected, then dies before the command arrives, leaving a
        // realistic stale selection entry for the stop path to filter.
        assert_eq!(
            world.select_ids(&[living, stale, hostile], PLAYER, false),
            2
        );
        world.unit_mut(living).unwrap().order = UnitOrder::AttackMove(Vec2::X * 50.0);
        world.unit_mut(living).unwrap().velocity = Vec2::X * 10.0;
        world.unit_mut(stale).unwrap().order = UnitOrder::Move(Vec2::X * 75.0);
        world.unit_mut(stale).unwrap().velocity = Vec2::X * 9.0;
        world.unit_mut(stale).unwrap().health = 0.0;
        world.unit_mut(hostile).unwrap().order = UnitOrder::Attack(living);
        world.unit_mut(hostile).unwrap().velocity = Vec2::X * 11.0;

        world.issue_stop();

        let living_unit = world.unit(living).unwrap();
        assert_eq!(living_unit.order, UnitOrder::Idle);
        assert_eq!(living_unit.velocity, Vec2::ZERO);
        assert!(living_unit.queued_orders.is_empty());

        let stale_unit = world.unit(stale).unwrap();
        assert_eq!(stale_unit.order, UnitOrder::Move(Vec2::X * 75.0));
        assert_eq!(stale_unit.velocity, Vec2::X * 9.0);

        let hostile_unit = world.unit(hostile).unwrap();
        assert_eq!(hostile_unit.order, UnitOrder::Attack(living));
        assert_eq!(hostile_unit.velocity, Vec2::X * 11.0);
    }

    #[test]
    fn stop_command_is_a_noop_with_empty_selection() {
        let mut world = RtsWorld::default();
        let id = world.spawn(PLAYER, Vec2::ZERO);
        world.unit_mut(id).unwrap().order = UnitOrder::Move(Vec2::X * 100.0);
        world.unit_mut(id).unwrap().velocity = Vec2::new(4.0, 2.0);
        world
            .unit_mut(id)
            .unwrap()
            .queued_orders
            .push_back(UnitOrder::Patrol(Vec2::ZERO, Vec2::X * 20.0));

        world.issue_stop();

        let unit = world.unit(id).unwrap();
        assert_eq!(unit.order, UnitOrder::Move(Vec2::X * 100.0));
        assert_eq!(unit.velocity, Vec2::new(4.0, 2.0));
        assert_eq!(unit.queued_orders.len(), 1);
    }

    #[test]
    fn movement_clamps_large_steps_to_destination() {
        let mut world = RtsWorld::default();
        let id = world.spawn(PLAYER, Vec2::ZERO);
        world.select_point(Vec2::ZERO, PLAYER, false);
        let destination = Vec2::new(50.0, 0.0);
        world.issue_move(destination, 0.0);

        // A ten-second tick is intentionally much larger than the travel
        // time. The unit should arrive exactly, not overshoot and oscillate.
        world.update(10.0);

        let unit = world.unit(id).unwrap();
        assert_eq!(unit.position, destination);
        assert_eq!(unit.velocity, Vec2::ZERO);
        assert_eq!(unit.order, UnitOrder::Idle);
    }

    #[test]
    fn attack_orders_hold_at_engagement_range() {
        let mut world = RtsWorld::default();
        let attacker = world.spawn(PLAYER, Vec2::ZERO);
        let target = world.spawn(FactionId(2), Vec2::new(500.0, 0.0));
        world.unit_mut(attacker).unwrap().engagement_range = 100.0;
        world.unit_mut(attacker).unwrap().order = UnitOrder::Attack(target);

        // A large step crosses the firing line. The unit should stop 100
        // world units short of the target and keep its attack order instead
        // of consuming it like a Move command.
        world.update(10.0);

        let unit = world.unit(attacker).unwrap();
        assert_eq!(unit.position, Vec2::new(400.0, 0.0));
        assert_eq!(unit.velocity, Vec2::ZERO);
        assert_eq!(unit.order, UnitOrder::Attack(target));
    }

    #[test]
    fn attack_orders_inside_engagement_range_do_not_snap_or_consume() {
        let mut world = RtsWorld::default();
        let attacker = world.spawn(PLAYER, Vec2::ZERO);
        let target = world.spawn(FactionId(2), Vec2::new(60.0, 0.0));
        world.unit_mut(attacker).unwrap().engagement_range = 100.0;
        world.unit_mut(attacker).unwrap().order = UnitOrder::Attack(target);

        world.update(1.0);

        let unit = world.unit(attacker).unwrap();
        assert_eq!(unit.position, Vec2::ZERO);
        assert_eq!(unit.velocity, Vec2::ZERO);
        assert_eq!(unit.order, UnitOrder::Attack(target));
    }

    #[test]
    fn stale_target_order_advances_to_queued_work() {
        let mut world = RtsWorld::default();
        let attacker = world.spawn(PLAYER, Vec2::ZERO);
        let target = world.spawn(FactionId(2), Vec2::new(100.0, 0.0));
        world.unit_mut(attacker).unwrap().order = UnitOrder::Attack(target);
        world
            .unit_mut(attacker)
            .unwrap()
            .queued_orders
            .push_back(UnitOrder::Move(Vec2::new(40.0, 0.0)));
        world.unit_mut(target).unwrap().health = 0.0;

        world.update(1.0);

        let unit = world.unit(attacker).unwrap();
        assert_eq!(unit.order, UnitOrder::Move(Vec2::new(40.0, 0.0)));
        assert_eq!(unit.velocity, Vec2::ZERO);
    }

    #[test]
    fn disconnected_power_components_resolve_independently() {
        let mut grid = PowerGrid::default();
        grid.add_node(PowerNode {
            id: PowerNodeId(0),
            supply: 3,
            demand: 0,
            online: true,
        });
        grid.add_node(PowerNode {
            id: PowerNodeId(1),
            supply: 0,
            demand: 2,
            online: true,
        });
        grid.add_node(PowerNode {
            id: PowerNodeId(2),
            supply: 0,
            demand: 1,
            online: true,
        });
        grid.link(PowerNodeId(0), PowerNodeId(1));
        assert!(grid.is_powered(PowerNodeId(1)));
        assert!(!grid.is_powered(PowerNodeId(2)));
        grid.link(PowerNodeId(1), PowerNodeId(2));
        assert!(grid.is_powered(PowerNodeId(2)));
    }

    #[test]
    fn control_groups_drop_destroyed_or_hostile_units_on_recall() {
        let mut world = RtsWorld::default();
        let first = world.spawn(PLAYER, Vec2::ZERO);
        world.spawn(PLAYER, Vec2::X);
        world.spawn(FactionId(2), Vec2::Y);
        world.select_bounds(
            Aabb::from_center_size(Vec2::ZERO, Vec2::splat(10.0)),
            PLAYER,
            false,
        );
        assert!(world.assign_control_group(1));
        world.unit_mut(first).unwrap().health = 0.0;
        assert!(world.recall_control_group(1, PLAYER));
        assert_eq!(world.selection().ids().len(), 1);
        assert!(!world.selection().contains(first));
    }

    #[test]
    fn support_targeting_chooses_lowest_health_ratio_deterministically() {
        let mut world = RtsWorld::default();
        let engineer = world.spawn(PLAYER, Vec2::ZERO);
        let first = world.spawn(PLAYER, Vec2::new(20.0, 0.0));
        let second = world.spawn(PLAYER, Vec2::new(30.0, 0.0));
        let enemy = world.spawn(FactionId(2), Vec2::new(10.0, 0.0));
        let distant = world.spawn(PLAYER, Vec2::new(500.0, 0.0));
        world.unit_mut(first).unwrap().health = 50.0;
        world.unit_mut(second).unwrap().health = 25.0;
        world.unit_mut(second).unwrap().max_health = 50.0;
        world.unit_mut(enemy).unwrap().health = 1.0;
        world.unit_mut(distant).unwrap().health = 1.0;

        assert_eq!(
            world.most_damaged_ally_in_range(engineer, PLAYER, 100.0),
            Some(first)
        );

        world.unit_mut(first).unwrap().health = 100.0;
        assert_eq!(
            world.most_damaged_ally_in_range(engineer, PLAYER, 100.0),
            Some(second)
        );
        world.unit_mut(second).unwrap().health = 0.0;
        assert_eq!(
            world.most_damaged_ally_in_range(engineer, PLAYER, 100.0),
            None
        );
    }
}
