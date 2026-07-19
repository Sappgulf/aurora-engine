//! Portable game settings and progress data.
//!
//! Aurora deliberately does not choose a storage backend here. Native games can
//! serialize this data to a file while web games can place the same payload in
//! local storage. Keeping the contract pure Rust makes save migration testable
//! on every target.

/// Increment this when the meaning of persisted data changes.
pub const SAVE_FORMAT_VERSION: u32 = 1;

/// User-facing render and accessibility preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameSettings {
    pub master_volume: f32,
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub ambience_volume: f32,
    pub ui_volume: f32,
    pub post_fx_enabled: bool,
    pub screen_shake_enabled: bool,
    pub reduced_motion: bool,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            music_volume: 0.8,
            sfx_volume: 0.85,
            ambience_volume: 0.7,
            ui_volume: 0.9,
            post_fx_enabled: true,
            screen_shake_enabled: true,
            reduced_motion: false,
        }
    }
}

impl GameSettings {
    /// Clamp untrusted or hand-edited values to values the runtime accepts.
    pub fn sanitize(&mut self) {
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        self.music_volume = self.music_volume.clamp(0.0, 1.0);
        self.sfx_volume = self.sfx_volume.clamp(0.0, 1.0);
        self.ambience_volume = self.ambience_volume.clamp(0.0, 1.0);
        self.ui_volume = self.ui_volume.clamp(0.0, 1.0);
    }
}

/// Data that persists between runs but is independent from an active scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveData {
    pub format_version: u32,
    pub settings: GameSettings,
    pub high_score: u64,
    pub runs_completed: u32,
}

impl Default for SaveData {
    fn default() -> Self {
        Self {
            format_version: SAVE_FORMAT_VERSION,
            settings: GameSettings::default(),
            high_score: 0,
            runs_completed: 0,
        }
    }
}

impl SaveData {
    /// Applies a completed run and returns whether it set a new high score.
    pub fn record_run(&mut self, score: u64) -> bool {
        self.runs_completed = self.runs_completed.saturating_add(1);
        if score > self.high_score {
            self.high_score = score;
            true
        } else {
            false
        }
    }

    /// Normalizes data from a storage adapter before the game uses it.
    pub fn sanitize(&mut self) {
        self.format_version = SAVE_FORMAT_VERSION;
        self.settings.sanitize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_clamp_and_runs_preserve_the_best_score() {
        let mut save = SaveData::default();
        save.settings.master_volume = 3.0;
        save.settings.sfx_volume = -1.0;
        save.sanitize();
        assert_eq!(save.settings.master_volume, 1.0);
        assert_eq!(save.settings.sfx_volume, 0.0);
        assert!(save.record_run(42));
        assert!(!save.record_run(10));
        assert_eq!((save.high_score, save.runs_completed), (42, 2));
    }
}
