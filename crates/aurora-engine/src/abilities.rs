//! Deterministic cooldown bookkeeping for gameplay abilities.
//!
//! Cooldowns are intentionally renderer-free. A fixed-step simulation can
//! arm, tick, and hash them identically on native and WASM without carrying
//! wall-clock or platform-specific timer state.

use std::collections::HashMap;

use crate::UnitId;

#[derive(Debug, Clone, Default)]
pub struct CooldownBook {
    remaining_millis: HashMap<UnitId, u32>,
}

impl CooldownBook {
    pub fn arm(&mut self, id: UnitId, seconds: f32) {
        let millis = (seconds.max(0.0) * 1_000.0).round() as u32;
        if millis == 0 {
            self.remaining_millis.remove(&id);
        } else {
            self.remaining_millis.insert(id, millis);
        }
    }

    pub fn remaining_millis(&self, id: UnitId) -> u32 {
        self.remaining_millis.get(&id).copied().unwrap_or(0)
    }

    pub fn remaining_seconds(&self, id: UnitId) -> f32 {
        self.remaining_millis(id) as f32 / 1_000.0
    }

    pub fn tick(&mut self, seconds: f32) {
        let millis = (seconds.max(0.0) * 1_000.0).round() as u32;
        if millis == 0 {
            return;
        }
        self.remaining_millis.retain(|_, remaining| {
            *remaining = remaining.saturating_sub(millis);
            *remaining > 0
        });
    }

    pub fn is_ready(&self, id: UnitId) -> bool {
        self.remaining_millis(id) == 0
    }

    /// Returns stable `(unit, milliseconds)` pairs for state hashing and
    /// diagnostics. HashMap iteration order is deliberately not exposed.
    pub fn entries_sorted(&self) -> Vec<(UnitId, u32)> {
        let mut entries: Vec<_> = self
            .remaining_millis
            .iter()
            .map(|(id, remaining)| (*id, *remaining))
            .collect();
        entries.sort_by_key(|(id, _)| id.0);
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::CooldownBook;
    use crate::UnitId;

    #[test]
    fn cooldowns_are_fixed_step_and_order_stable() {
        let mut book = CooldownBook::default();
        book.arm(UnitId(7), 1.25);
        book.arm(UnitId(2), 0.5);
        assert!(!book.is_ready(UnitId(7)));
        assert_eq!(book.remaining_millis(UnitId(7)), 1_250);
        assert_eq!(
            book.entries_sorted(),
            vec![(UnitId(2), 500), (UnitId(7), 1_250)]
        );

        book.tick(0.5);
        assert!(book.is_ready(UnitId(2)));
        assert_eq!(book.remaining_millis(UnitId(7)), 750);
        book.tick(0.75);
        assert!(book.is_ready(UnitId(7)));
    }
}
