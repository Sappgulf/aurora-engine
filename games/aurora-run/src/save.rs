//! Aurora Run's deliberately small, game-owned progression schema.

use aurora_engine::{SaveEnvelope, SaveError, SaveStore};
use serde::{Deserialize, Serialize};

pub const SAVE_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSettings {
    pub post_fx_enabled: bool,
    pub reduced_motion: bool,
}
impl Default for RunSettings {
    fn default() -> Self {
        Self {
            post_fx_enabled: true,
            reduced_motion: false,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunSave {
    pub settings: RunSettings,
    pub high_score: u64,
    pub runs_completed: u32,
}
impl RunSave {
    pub fn record_run(&mut self, score: u64) -> bool {
        self.runs_completed = self.runs_completed.saturating_add(1);
        if score > self.high_score {
            self.high_score = score;
            true
        } else {
            false
        }
    }
}
pub type RunStore = SaveStore<RunSave>;
pub fn load(store: &RunStore) -> Result<Option<RunSave>, SaveError> {
    store
        .load_with(SAVE_VERSION, Ok)
        .map(|save| save.map(|envelope| envelope.payload))
}
pub fn envelope(data: RunSave) -> SaveEnvelope<RunSave> {
    SaveEnvelope::new(SAVE_VERSION, data)
}
