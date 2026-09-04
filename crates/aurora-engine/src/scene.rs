//! Lightweight, renderer-independent entity storage.
//!
//! `Scene` deliberately owns simulation values only. Rendering stays in the
//! renderer, so a scene can be tested, saved, or simulated without a GPU.

use std::collections::HashMap;

use glam::Vec2;

/// Stable entity identifier with a generation counter.
///
/// An ID becomes invalid when its entity is removed, even if the underlying
/// slot is later reused for another entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId {
    index: u32,
    generation: u32,
}

impl EntityId {
    /// Zero-based slot index. Useful for compact external component stores.
    pub const fn index(self) -> u32 {
        self.index
    }
}

#[derive(Debug)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Minimal spatial contract so hierarchy propagation can compose offsets.
///
/// Implemented by any component type that carries a world position; games
/// usually implement it once for their sprite/state tuple.
pub trait Positioned {
    fn position(&self) -> Vec2;
    fn set_position(&mut self, position: Vec2);
}

/// Compact storage for one simulation value per entity.
///
/// Games can use several `Scene`s keyed by the same `EntityId` later, but a
/// single typed scene is a clear, small foundation for Aurora's first games.
///
/// Parent links (`attach`) make children follow parent movement deterministically:
/// [`Scene::propagate`], run each fixed tick in stable slot order, applies the
/// parents' per-tick displacement to every descendant (deep chains work).
/// Loops are rejected at attach time.
#[derive(Debug)]
pub struct Scene<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
    /// Child -> parent, generation-checked on use.
    parents: HashMap<EntityId, EntityId>,
}

impl<T> Default for Scene<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Scene<T> {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
            parents: HashMap::new(),
        }
    }

    /// Insert a simulation value and return its stable entity ID.
    pub fn spawn(&mut self, value: T) -> EntityId {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.value.is_none());
            slot.value = Some(value);
            return EntityId {
                index,
                generation: slot.generation,
            };
        }

        let index = self.slots.len() as u32;
        self.slots.push(Slot {
            generation: 0,
            value: Some(value),
        });
        EntityId {
            index,
            generation: 0,
        }
    }

    pub fn contains(&self, entity: EntityId) -> bool {
        self.slot(entity).is_some_and(|slot| slot.value.is_some())
    }

    pub fn get(&self, entity: EntityId) -> Option<&T> {
        self.slot(entity)?.value.as_ref()
    }

    pub fn get_mut(&mut self, entity: EntityId) -> Option<&mut T> {
        self.slot_mut(entity)?.value.as_mut()
    }

    /// Remove an entity. Returns its simulation value when the ID is current.
    pub fn despawn(&mut self, entity: EntityId) -> Option<T> {
        let slot = self.slot_mut(entity)?;
        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(entity.index);
        self.len -= 1;
        Some(value)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value.as_ref().map(|value| {
                (
                    EntityId {
                        index: index as u32,
                        generation: slot.generation,
                    },
                    value,
                )
            })
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (EntityId, &mut T)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| {
                let generation = slot.generation;
                slot.value.as_mut().map(|value| {
                    (
                        EntityId {
                            index: index as u32,
                            generation,
                        },
                        value,
                    )
                })
            })
    }

    fn slot(&self, entity: EntityId) -> Option<&Slot<T>> {
        let slot = self.slots.get(entity.index as usize)?;
        (slot.generation == entity.generation).then_some(slot)
    }

    fn slot_mut(&mut self, entity: EntityId) -> Option<&mut Slot<T>> {
        let slot = self.slots.get_mut(entity.index as usize)?;
        (slot.generation == entity.generation).then_some(slot)
    }

    /// Links `child` to follow `parent` (deep chains allowed). Re-attaching
    /// replaces the previous link; cycles are rejected and reported.
    pub fn attach(&mut self, child: EntityId, parent: EntityId) -> Result<(), HierarchyError> {
        if child == parent {
            return Err(HierarchyError::SelfParent);
        }
        // Reject links onto dead entities up front for a crisp contract.
        let valid = |scene: &Self, id: EntityId| scene.contains(id);
        if !valid(self, child) || !valid(self, parent) {
            return Err(HierarchyError::DeadEntity);
        }
        // Cycle walk: can `parent` reach itself through the new edge?
        let mut cursor = Some(parent);
        while let Some(current) = cursor {
            if current == child {
                return Err(HierarchyError::Cycle);
            }
            cursor = self.parents.get(&current).copied();
        }
        self.parents.insert(child, parent);
        Ok(())
    }

    /// Removes any parent link on `child`.
    pub fn detach(&mut self, child: EntityId) -> bool {
        self.parents.remove(&child).is_some()
    }

    pub fn parent_of(&self, child: EntityId) -> Option<EntityId> {
        // Links referencing stale entities read as absent.
        let parent = *self.parents.get(&child)?;
        self.contains(parent).then_some(parent)
    }

    /// Applies game-reported per-tick movement to whole descendant chains.
    ///
    /// `moved` maps *live* entities to the world-space offset they shifted
    /// this tick (movers, ferries, bosses). Each linked child receives its
    /// ancestors' accumulated delta exactly once per tick. Chains resolve
    /// deterministically when parents occupy lower slots than children (the
    /// spawn-order convention); deeper nesting formed the other way converges
    /// on the next tick, lag-bounded, never diverging.
    pub fn propagate(&mut self, moved: &HashMap<EntityId, Vec2>)
    where
        T: Positioned,
    {
        if self.parents.is_empty() || moved.is_empty() {
            return;
        }
        let mut links: Vec<(EntityId, EntityId)> = self
            .parents
            .iter()
            .filter(|&(&child, &parent)| self.contains(child) && self.contains(parent))
            .map(|(&child, &parent)| (child, parent))
            .collect();
        // Ascending child order keeps chains settle-fast for the spawn-order
        // convention (parents before children); other orders still converge
        // over rounds below.
        links.sort_unstable_by_key(|&(child, _)| child);

        let mut settled: HashMap<EntityId, Vec2> = HashMap::new();
        for _ in 0..8 {
            let mut changed = false;
            for &(child, parent) in &links {
                let Some(inherited) = moved
                    .get(&parent)
                    .copied()
                    .or_else(|| settled.get(&parent).copied())
                else {
                    continue;
                };
                let next = match settled.get(&child) {
                    Some(current) if *current == inherited => continue,
                    Some(_) => inherited,
                    None => inherited,
                };
                if settled.get(&child) != Some(&next) {
                    settled.insert(child, next);
                    changed = true;
                }
                let _ = changed;
            }
            if !changed {
                break;
            }
        }

        for (child, delta) in settled {
            if let Some(value) = self.get_mut(child) {
                let next = value.position() + delta;
                value.set_position(next);
            }
        }
    }
}

/// Failure modes of [`Scene::attach`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyError {
    SelfParent,
    DeadEntity,
    Cycle,
}

impl std::fmt::Display for HierarchyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfParent => write!(f, "an entity cannot parent itself"),
            Self::DeadEntity => write!(f, "hierarchy link references despawned entity"),
            Self::Cycle => write!(f, "attach would create a parenting cycle"),
        }
    }
}

impl std::error::Error for HierarchyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_entity_id_cannot_access_reused_slot() {
        let mut scene = Scene::new();
        let original = scene.spawn("old");
        assert_eq!(scene.despawn(original), Some("old"));

        let replacement = scene.spawn("new");
        assert_eq!(original.index(), replacement.index());
        assert_ne!(original, replacement);
        assert!(!scene.contains(original));
        assert_eq!(scene.get(replacement), Some(&"new"));
    }
}

#[cfg(test)]
mod hierarchy_tests {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct Node {
        position: Vec2,
    }

    impl Positioned for Node {
        fn position(&self) -> Vec2 {
            self.position
        }
        fn set_position(&mut self, position: Vec2) {
            self.position = position;
        }
    }

    #[test]
    fn children_follow_parent_deltas_exactly_once_per_tick() {
        let mut scene = Scene::new();
        let parent = scene.spawn(Node {
            position: Vec2::ZERO,
        });
        let child = scene.spawn(Node {
            position: Vec2::new(40.0, -10.0),
        });
        let grandchild = scene.spawn(Node {
            position: Vec2::new(-4.0, 8.0),
        });
        scene.attach(child, parent).expect("linear chain");
        scene.attach(grandchild, child).expect("deep chain");

        let moved = HashMap::from([(parent, Vec2::new(3.0, 0.0))]);
        scene.propagate(&moved);

        assert_eq!(scene.get(child).unwrap().position, Vec2::new(43.0, -10.0));
        assert_eq!(
            scene.get(grandchild).unwrap().position,
            Vec2::new(-1.0, 8.0),
            "grandchild inherits through the middle link"
        );

        // Idempotent per tick: replaying the same map must not double-move.
        scene.propagate(&moved);
        assert_eq!(scene.get(child).unwrap().position, Vec2::new(46.0, -10.0));
    }

    #[test]
    fn rejections_cover_self_dead_and_cycle_cases() {
        let mut scene = Scene::new();
        let a = scene.spawn(Node {
            position: Vec2::ZERO,
        });
        let b = scene.spawn(Node {
            position: Vec2::ONE,
        });
        scene.attach(b, a).unwrap();

        assert_eq!(scene.attach(a, a), Err(HierarchyError::SelfParent));
        assert_eq!(scene.attach(a, b), Err(HierarchyError::Cycle));

        scene.despawn(a);
        let ghost_child = EntityId {
            index: a.index(),
            generation: 999,
        };
        assert_eq!(
            scene.attach(ghost_child, b),
            Err(HierarchyError::DeadEntity)
        );
    }

    #[test]
    fn stale_links_ignore_despawned_sides_and_detach_clears() {
        let mut scene = Scene::new();
        let parent = scene.spawn(Node {
            position: Vec2::ZERO,
        });
        let child = scene.spawn(Node {
            position: Vec2::ZERO,
        });
        scene.attach(child, parent).unwrap();
        scene.despawn(parent);

        assert_eq!(scene.parent_of(child), None, "stale parents read absent");
        // And propagation with only the stale pair does nothing.
        let moved = HashMap::from([(parent, Vec2::splat(50.0))]);
        scene.propagate(&moved);
        assert_eq!(scene.get(child).unwrap().position, Vec2::ZERO);

        let new_parent = scene.spawn(Node {
            position: Vec2::ZERO,
        });
        scene.attach(child, new_parent).unwrap();
        assert!(scene.detach(child));
        assert!(!scene.detach(child));
    }

    #[test]
    fn chains_settle_multi_round_when_children_precede_parents() {
        let mut scene = Scene::new();
        // Spawn child FIRST so its slot sorts before its parent.
        let child = scene.spawn(Node {
            position: Vec2::ZERO,
        });
        let parent = scene.spawn(Node {
            position: Vec2::ZERO,
        });
        scene.attach(child, parent).unwrap();

        scene.propagate(&HashMap::from([(parent, Vec2::new(7.5, 2.25))]));
        assert_eq!(
            scene.get(child).unwrap().position,
            Vec2::new(7.5, 2.25),
            "reverse-order chains still settle"
        );
    }
}
