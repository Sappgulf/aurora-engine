//! A target-neutral asset loading state machine.
//!
//! File I/O, browser fetches, and hot reload all feed this queue, while games
//! can present one consistent loading screen from the state alone.

use std::collections::{BTreeMap, VecDeque};

use crate::assets::{AssetKey, AssetManifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetLoadState {
    Queued,
    Loading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetLoadEntry {
    pub state: AssetLoadState,
    pub error: Option<String>,
}

/// Deterministic queue for an asset manifest. Adapters pop a key, perform I/O,
/// then call `mark_ready` or `mark_failed` from their completion callback.
#[derive(Debug, Default, Clone)]
pub struct AssetLoadQueue {
    entries: BTreeMap<AssetKey, AssetLoadEntry>,
    pending: VecDeque<AssetKey>,
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
        self.pending.push_back(key);
        true
    }

    /// Moves one queued asset into Loading and returns its stable key.
    pub fn begin_next(&mut self) -> Option<AssetKey> {
        let key = self.pending.pop_front()?;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.state = AssetLoadState::Loading;
        }
        Some(key)
    }

    pub fn mark_ready(&mut self, key: &AssetKey) -> bool {
        self.set_state(key, AssetLoadState::Ready, None)
    }

    pub fn mark_failed(&mut self, key: &AssetKey, error: impl Into<String>) -> bool {
        self.set_state(key, AssetLoadState::Failed, Some(error.into()))
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
    }
}
