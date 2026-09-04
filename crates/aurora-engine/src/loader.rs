//! A target-neutral asset loading state machine.
//!
//! File I/O, browser fetches, and hot reload all feed this queue, while games
//! can present one consistent loading screen from the state alone.

use std::collections::{BTreeMap, BTreeSet};

use crate::assets::{AssetKey, AssetManifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetLoadState {
    Queued,
    Loading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetPriority {
    Critical,
    Gameplay,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetLoadEntry {
    pub state: AssetLoadState,
    pub error: Option<String>,
}

/// Deterministic queue for an asset manifest. Adapters pop a key, perform I/O,
/// then call `mark_ready` or `mark_failed` from their completion callback.
#[derive(Debug, Clone)]
pub struct AssetLoadQueue {
    entries: BTreeMap<AssetKey, AssetLoadEntry>,
    priorities: BTreeMap<AssetKey, AssetPriority>,
    pending: BTreeSet<(AssetPriority, AssetKey)>,
    residency_budget: usize,
    resident_bytes: usize,
    resident: BTreeMap<AssetKey, usize>,
    rejected_optional_bytes: usize,
}

impl Default for AssetLoadQueue {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            priorities: BTreeMap::new(),
            pending: BTreeSet::new(),
            residency_budget: 256 * 1024 * 1024,
            resident_bytes: 0,
            resident: BTreeMap::new(),
            rejected_optional_bytes: 0,
        }
    }
}

impl AssetLoadQueue {
    pub fn from_manifest(manifest: &AssetManifest) -> Self {
        let mut queue = Self::default();
        for (key, _) in manifest.iter() {
            queue.enqueue(key.clone());
        }
        queue
    }

    pub fn enqueue(&mut self, key: AssetKey) -> bool {
        self.enqueue_with_priority(key, AssetPriority::Gameplay)
    }

    pub fn enqueue_with_priority(&mut self, key: AssetKey, priority: AssetPriority) -> bool {
        if self.entries.contains_key(&key) {
            return false;
        }
        self.entries.insert(
            key.clone(),
            AssetLoadEntry {
                state: AssetLoadState::Queued,
                error: None,
            },
        );
        self.priorities.insert(key.clone(), priority);
        self.pending.insert((priority, key));
        true
    }

    /// Moves one queued asset into Loading and returns its stable key.
    pub fn begin_next(&mut self) -> Option<AssetKey> {
        let pending = self.pending.iter().next().cloned()?;
        self.pending.remove(&pending);
        let (_, key) = pending;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.state = AssetLoadState::Loading;
        }
        Some(key)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn mark_ready(&mut self, key: &AssetKey) -> bool {
        self.set_state(key, AssetLoadState::Ready, None)
    }

    pub fn mark_failed(&mut self, key: &AssetKey, error: impl Into<String>) -> bool {
        self.set_state(key, AssetLoadState::Failed, Some(error.into()))
    }

    /// Completes all work owned by a synchronous startup loader. Entries that
    /// already failed remain failed so diagnostics preserve the real error;
    /// queued/loading entries become ready and cannot be returned again.
    pub fn mark_all_ready(&mut self) -> usize {
        let mut transitioned = 0;
        for entry in self.entries.values_mut() {
            if matches!(
                entry.state,
                AssetLoadState::Queued | AssetLoadState::Loading
            ) {
                entry.state = AssetLoadState::Ready;
                entry.error = None;
                transitioned += 1;
            }
        }
        self.pending.clear();
        transitioned
    }

    pub fn state(&self, key: &AssetKey) -> Option<AssetLoadState> {
        self.entries.get(key).map(|entry| entry.state)
    }

    pub fn entry(&self, key: &AssetKey) -> Option<&AssetLoadEntry> {
        self.entries.get(key)
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn ready_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.state == AssetLoadState::Ready)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.state == AssetLoadState::Failed)
            .count()
    }

    pub fn is_complete(&self) -> bool {
        self.total() > 0 && self.ready_count() + self.failed_count() == self.total()
    }

    pub fn progress(&self) -> f32 {
        if self.total() == 0 {
            1.0
        } else {
            (self.ready_count() + self.failed_count()) as f32 / self.total() as f32
        }
    }

    pub fn set_residency_budget(&mut self, bytes: usize) {
        self.residency_budget = bytes;
        self.evict_to_budget();
    }

    pub fn residency_budget(&self) -> usize {
        self.residency_budget
    }

    pub fn set_resident_bytes(&mut self, bytes: usize) {
        self.resident.clear();
        self.resident_bytes = bytes.min(self.residency_budget);
    }

    pub fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }

    pub fn rejected_optional_bytes(&self) -> usize {
        self.rejected_optional_bytes
    }

    pub fn admit_resident_bytes(&mut self, key: &AssetKey, bytes: usize, optional: bool) -> bool {
        let previous = self.resident.remove(key).unwrap_or(0);
        self.resident_bytes = self.resident_bytes.saturating_sub(previous);
        let fits = self
            .resident_bytes
            .checked_add(bytes)
            .is_some_and(|total| total <= self.residency_budget);
        if !fits {
            if previous > 0 {
                self.resident.insert(key.clone(), previous);
                self.resident_bytes = self.resident_bytes.saturating_add(previous);
            }
            if optional {
                self.rejected_optional_bytes = self.rejected_optional_bytes.saturating_add(bytes);
            }
            return false;
        }
        if bytes > 0 {
            self.resident.insert(key.clone(), bytes);
            self.resident_bytes = self.resident_bytes.saturating_add(bytes);
        }
        true
    }

    fn evict_to_budget(&mut self) {
        if self.resident_bytes <= self.residency_budget {
            return;
        }
        let mut candidates: Vec<(AssetPriority, AssetKey)> = self
            .resident
            .keys()
            .map(|key| {
                (
                    self.priorities
                        .get(key)
                        .copied()
                        .unwrap_or(AssetPriority::Optional),
                    key.clone(),
                )
            })
            .collect();
        candidates.sort_by(|left, right| right.cmp(left));
        for (_, key) in candidates {
            if self.resident_bytes <= self.residency_budget {
                break;
            }
            if let Some(bytes) = self.resident.remove(&key) {
                self.resident_bytes = self.resident_bytes.saturating_sub(bytes);
            }
        }
    }

    fn set_state(&mut self, key: &AssetKey, state: AssetLoadState, error: Option<String>) -> bool {
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        entry.state = state;
        entry.error = error;
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::assets::{AssetKind, AssetManifest};

    use super::*;

    #[test]
    fn queue_reports_progress_for_success_and_failure() {
        let mut manifest = AssetManifest::new();
        let sprite = AssetKey::new("sprite.runner").unwrap();
        let audio = AssetKey::new("audio.collect").unwrap();
        manifest
            .insert(sprite.clone(), AssetKind::Texture, "runner.png")
            .unwrap();
        manifest
            .insert(audio.clone(), AssetKind::Audio, "collect.ogg")
            .unwrap();
        let mut queue = AssetLoadQueue::from_manifest(&manifest);
        assert_eq!(queue.begin_next(), Some(audio.clone()));
        assert!(queue.mark_failed(&audio, "unsupported codec"));
        assert_eq!(queue.begin_next(), Some(sprite.clone()));
        assert!(queue.mark_ready(&sprite));
        assert!(queue.is_complete());
        assert_eq!(queue.failed_count(), 1);
        assert_eq!(queue.progress(), 1.0);

        let mut startup_queue = AssetLoadQueue::from_manifest(&manifest);
        assert_eq!(startup_queue.mark_all_ready(), 2);
        assert_eq!(startup_queue.ready_count(), startup_queue.total());
        assert!(startup_queue.is_complete());
        assert!(startup_queue.begin_next().is_none());
    }

    #[test]
    fn asset_priority_is_deterministic_and_optional_work_is_rejected_first() {
        let mut queue = AssetLoadQueue::default();
        let optional = AssetKey::new("optional.cover").unwrap();
        let critical = AssetKey::new("critical.player").unwrap();
        queue.enqueue_with_priority(optional.clone(), AssetPriority::Optional);
        queue.enqueue_with_priority(critical.clone(), AssetPriority::Critical);
        assert_eq!(queue.begin_next(), Some(critical));
        assert_eq!(queue.pending_count(), 1);
    }

    #[test]
    fn residency_budget_never_exceeds_the_configured_bytes() {
        let mut queue = AssetLoadQueue::default();
        queue.set_residency_budget(1024);
        let gameplay = AssetKey::new("gameplay.hero").unwrap();
        let optional = AssetKey::new("optional.fx").unwrap();
        queue.enqueue(gameplay.clone());
        queue.enqueue_with_priority(optional.clone(), AssetPriority::Optional);
        assert!(queue.admit_resident_bytes(&gameplay, 768, false));
        assert!(!queue.admit_resident_bytes(&optional, 512, true));
        assert_eq!(queue.resident_bytes(), 768);
    }
}
