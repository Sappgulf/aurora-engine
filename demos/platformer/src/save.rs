//! Persistent platformer progress: best clear times per level.
//!
//! Uses the engine's versioned [`SaveStore`] so native builds land in the OS
//! data directory and web builds in localStorage, with the same payload.

use aurora_engine::{SaveEnvelope, SaveStore};
use serde::{Deserialize, Serialize};

use crate::game_core::replay::ReplayLog;
use crate::game_core::CoreIntent;

pub const FORMAT_VERSION: u32 = 1;

/// Best clear time per shipped level, index-aligned with the level list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BestTimes {
    pub times: Vec<Option<f32>>,
}

impl BestTimes {
    pub fn best(&self, level_index: usize) -> Option<f32> {
        self.times
            .get(level_index)
            .and_then(|slot| slot.filter(|time| time.is_finite() && *time > 0.0))
    }
}

/// Owns the save slot and applies new clear times.
pub struct Progress {
    store: SaveStore<BestTimes>,
    times: BestTimes,
    pub ghosts: GhostStore,
}

impl Progress {
    /// Loads from the platform's standard save location; corrupt or missing
    /// data degrades to a fresh profile (best times are never worth a crash).
    pub fn load() -> Self {
        let store: SaveStore<BestTimes> = SaveStore::new("aurora-platformer", "best-times");
        let ghosts = GhostStore::load();
        let times = store
            .load_with(FORMAT_VERSION, Ok)
            .ok()
            .flatten()
            .map(|envelope| envelope.payload)
            .unwrap_or_default();
        Self {
            store,
            times,
            ghosts,
        }
    }

    /// Test seam: same logic against an explicit file path.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_path(path: &std::path::Path) -> Self {
        let store = SaveStore::with_path("aurora-platformer", "best-times", path);
        let ghosts = GhostStore::load();
        let times = store
            .load_with(FORMAT_VERSION, Ok)
            .ok()
            .flatten()
            .map(|envelope| envelope.payload)
            .unwrap_or_default();
        Self {
            store,
            times,
            ghosts,
        }
    }

    pub fn best(&self, level_index: usize) -> Option<f32> {
        self.times.best(level_index)
    }

    /// Records a clear time; returns `true` when it is a new best.
    pub fn record(&mut self, level_index: usize, seconds: f32) -> bool {
        if !seconds.is_finite() || seconds <= 0.0 {
            return false;
        }
        if self.times.times.len() <= level_index {
            self.times.times.resize(level_index + 1, None);
        }
        let is_best = match self.times.times[level_index] {
            Some(current) if current <= seconds => false,
            _ => {
                self.times.times[level_index] = Some(seconds);
                true
            }
        };
        if is_best {
            self.persist();
        }
        is_best
    }

    fn persist(&self) {
        let envelope = SaveEnvelope::new(FORMAT_VERSION, self.times.clone());
        if let Err(error) = self.store.save(&envelope) {
            log::warn!("could not persist best times: {error}");
        }
    }
}

/// Shape-contract note: `load_with` already tolerates corrupt or newer
/// payloads (`Serialization` / `NewerFormat` / `Io`) by degrading to a fresh
/// profile in [`Progress::load`]; best times are never worth a crash.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn temp_store(label: &str) -> (Progress, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "aurora-platformer-progress-{label}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        (Progress::with_path(&path), path)
    }

    #[test]
    fn records_improvements_and_reports_new_bests() {
        let (mut progress, _path) = temp_store("improve");
        assert!(progress.best(0).is_none());

        assert!(progress.record(0, 20.0), "first clear is a new best");
        assert_eq!(progress.best(0), Some(20.0));
        assert!(!progress.record(0, 25.0), "slower runs are not bests");
        assert!(progress.record(0, 15.5), "faster runs are bests");
        assert_eq!(progress.best(0), Some(15.5));

        assert!(!progress.record(0, f32::NAN), "garbage times rejected");
        assert!(!progress.record(0, 0.0));
        assert_eq!(progress.best(0), Some(15.5));
    }

    #[test]
    fn best_times_survive_a_round_trip_through_disk() {
        let path = std::env::temp_dir().join(format!(
            "aurora-platformer-progress-roundtrip-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        {
            let mut progress = Progress::with_path(&path);
            progress.record(1, 31.25);
            progress.record(2, 44.0);
        }
        let reloaded = Progress::with_path(&path);
        assert_eq!(reloaded.best(1), Some(31.25));
        assert_eq!(reloaded.best(2), Some(44.0));
        assert_eq!(reloaded.best(0), None);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn corrupt_saves_degrade_to_fresh() {
        let path = std::env::temp_dir().join(format!(
            "aurora-platformer-progress-corrupt-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::write(&path, b"this is not json").unwrap();
        let progress = Progress::with_path(&path);
        assert_eq!(progress.best(0), None);
        std::fs::remove_file(&path).ok();
    }
}

/// Persistent ghost runs: the recorded intents of each level's best clear.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GhostRuns {
    pub runs: Vec<Option<Vec<CoreIntent>>>,
}

pub struct GhostStore {
    store: SaveStore<GhostRuns>,
    runs: GhostRuns,
}

impl GhostStore {
    pub fn load() -> Self {
        let store: SaveStore<GhostRuns> = SaveStore::new("aurora-platformer", "ghosts");
        let runs = store
            .load_with(1, Ok)
            .ok()
            .flatten()
            .map(|envelope| envelope.payload)
            .unwrap_or_default();
        Self { store, runs }
    }

    pub fn get(&self, level_index: usize) -> Option<&[CoreIntent]> {
        self.runs
            .runs
            .get(level_index)
            .and_then(|slot| slot.as_deref())
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub fn with_path_for_test(path: &std::path::Path) -> Self {
        let store = SaveStore::with_path("aurora-platformer", "ghosts", path);
        let runs = store
            .load_with(1, Ok)
            .ok()
            .flatten()
            .map(|envelope| envelope.payload)
            .unwrap_or_default();
        Self { store, runs }
    }

    /// Persists a run; returns `true` when it replaced an existing ghost.
    pub fn record(&mut self, level_index: usize, log: &ReplayLog) -> bool {
        if log.is_empty() {
            return false;
        }
        if self.runs.runs.len() <= level_index {
            self.runs.runs.resize(level_index + 1, None);
        }
        let replaced = self.runs.runs[level_index].is_some();
        self.runs.runs[level_index] = Some(log.intents.clone());
        let envelope = SaveEnvelope::new(1, self.runs.clone());
        if let Err(error) = self.store.save(&envelope) {
            log::warn!("could not persist ghost run: {error}");
        }
        replaced
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ghost_tests {
    use super::*;
    use crate::game_core::CoreIntent;

    #[test]
    fn ghost_runs_round_trip_through_disk() {
        let path = std::env::temp_dir().join(format!(
            "aurora-ghosts-roundtrip-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        {
            let mut ghosts = GhostStore::with_path_for_test(&path);
            let mut log = ReplayLog::new();
            log.record(CoreIntent {
                move_x: 1.0,
                jump_pressed: true,
                ..Default::default()
            });
            log.record(CoreIntent {
                move_x: -0.5,
                ..Default::default()
            });
            ghosts.record(0, &log);
        }
        let reloaded = GhostStore::with_path_for_test(&path);
        let run = reloaded.get(0).expect("ghost persisted");
        assert_eq!(run.len(), 2);
        assert_eq!(run[0].move_x, 1.0);
        assert!(run[0].jump_pressed);
        assert_eq!(run[1].move_x, -0.5);
        std::fs::remove_file(&path).ok();
    }
}
