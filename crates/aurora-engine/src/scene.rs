//! Lightweight, renderer-independent entity storage.
//!
//! `Scene` deliberately owns simulation values only. Rendering stays in the
//! renderer, so a scene can be tested, saved, or simulated without a GPU.

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

/// Compact storage for one simulation value per entity.
///
/// Games can use several `Scene`s keyed by the same `EntityId` later, but a
/// single typed scene is a clear, small foundation for Aurora's first games.
#[derive(Debug)]
pub struct Scene<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Default for Scene<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Scene<T> {
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
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
}

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
