//! Portable game settings and progress data.
//!
//! The same [`SaveStore`] API uses a user-data file on native targets and
//! browser `localStorage` on wasm. The payload remains plain JSON, making it
//! simple to inspect, back up, and migrate.

use serde::{Deserialize, Serialize};
use std::fmt;

#[cfg(not(target_arch = "wasm32"))]
use std::{fs, path::PathBuf};

/// Increment this when the meaning of persisted data changes.
pub const SAVE_FORMAT_VERSION: u32 = 1;

/// The default slot used by games that do not need multiple player profiles.
pub const DEFAULT_SAVE_SLOT: &str = "default";

/// User-facing render and accessibility preferences.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Errors returned by a storage backend. Callers can offer a "reset save"
/// action for any error without crashing the running game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    Io(String),
    Serialization(String),
    StorageUnavailable(String),
    NewerFormat { found: u32, supported: u32 },
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(f, "save I/O error: {message}"),
            Self::Serialization(message) => write!(f, "invalid save data: {message}"),
            Self::StorageUnavailable(message) => write!(f, "save storage unavailable: {message}"),
            Self::NewerFormat { found, supported } => write!(
                f,
                "save format {found} is newer than this engine supports ({supported})"
            ),
        }
    }
}

impl std::error::Error for SaveError {}

/// A named, cross-platform save slot.
///
/// Native builds store `slot.json` in the platform's application-data folder.
/// Browser builds store the same JSON under `aurora-engine:<slot>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStore {
    slot: String,
    #[cfg(not(target_arch = "wasm32"))]
    path: PathBuf,
}

impl Default for SaveStore {
    fn default() -> Self {
        Self::new(DEFAULT_SAVE_SLOT)
    }
}

impl SaveStore {
    /// Creates a store for a stable, application-defined slot name.
    pub fn new(slot: impl Into<String>) -> Self {
        let slot = sanitize_slot(slot.into());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = native_save_directory().join(format!("{slot}.json"));
            Self { slot, path }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self { slot }
        }
    }

    /// Returns the normalized slot name. This is safe to show in diagnostics.
    pub fn slot(&self) -> &str {
        &self.slot
    }

    /// Loads a save. `Ok(None)` means the player has not saved in this slot.
    pub fn load(&self) -> Result<Option<SaveData>, SaveError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match fs::read(&self.path) {
                Ok(bytes) => decode_save(&bytes).map(Some),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(SaveError::Io(error.to_string())),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let storage = browser_storage()?;
            match storage
                .get_item(&browser_key(&self.slot))
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?
            {
                Some(json) => decode_save(json.as_bytes()).map(Some),
                None => Ok(None),
            }
        }
    }

    /// Atomically persists a sanitized save on native targets.
    pub fn save(&self, save: &SaveData) -> Result<(), SaveError> {
        let mut save = save.clone();
        save.sanitize();
        let bytes = serde_json::to_vec_pretty(&save)
            .map_err(|error| SaveError::Serialization(error.to_string()))?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let parent = self
                .path
                .parent()
                .ok_or_else(|| SaveError::Io("save path has no parent directory".to_owned()))?;
            fs::create_dir_all(parent).map_err(|error| SaveError::Io(error.to_string()))?;
            let temporary = self.path.with_extension("json.tmp");
            fs::write(&temporary, bytes).map_err(|error| SaveError::Io(error.to_string()))?;
            fs::rename(&temporary, &self.path).map_err(|error| SaveError::Io(error.to_string()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let json = String::from_utf8(bytes)
                .map_err(|error| SaveError::Serialization(error.to_string()))?;
            browser_storage()?
                .set_item(&browser_key(&self.slot), &json)
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))
        }
    }

    /// Removes this slot. It is safe to call if the slot does not yet exist.
    pub fn clear(&self) -> Result<(), SaveError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(SaveError::Io(error.to_string())),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            browser_storage()?
                .remove_item(&browser_key(&self.slot))
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))
        }
    }

    /// Overrides the native path, useful for editor tools and isolated tests.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_path(slot: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            slot: sanitize_slot(slot.into()),
            path: path.into(),
        }
    }

    /// The native file location used by this slot.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

fn decode_save(bytes: &[u8]) -> Result<SaveData, SaveError> {
    let mut save: SaveData = serde_json::from_slice(bytes)
        .map_err(|error| SaveError::Serialization(error.to_string()))?;
    if save.format_version > SAVE_FORMAT_VERSION {
        return Err(SaveError::NewerFormat {
            found: save.format_version,
            supported: SAVE_FORMAT_VERSION,
        });
    }
    save.sanitize();
    Ok(save)
}

fn sanitize_slot(slot: String) -> String {
    let mut normalized: String = slot
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '-',
        })
        .collect();
    normalized.truncate(64);
    if normalized.trim_matches('-').is_empty() {
        DEFAULT_SAVE_SLOT.to_owned()
    } else {
        normalized
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_save_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("AURORA_SAVE_DIR") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Aurora Engine");
    }
    #[cfg(target_os = "windows")]
    if let Some(app_data) = std::env::var_os("APPDATA") {
        return PathBuf::from(app_data).join("Aurora Engine");
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aurora-engine")
}

#[cfg(target_arch = "wasm32")]
fn browser_key(slot: &str) -> String {
    format!("aurora-engine:{slot}")
}

#[cfg(target_arch = "wasm32")]
fn browser_storage() -> Result<web_sys::Storage, SaveError> {
    web_sys::window()
        .ok_or_else(|| SaveError::StorageUnavailable("browser window is unavailable".to_owned()))?
        .local_storage()
        .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?
        .ok_or_else(|| SaveError::StorageUnavailable("localStorage is unavailable".to_owned()))
}

#[cfg(target_arch = "wasm32")]
fn js_error(error: &wasm_bindgen::JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
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

    #[test]
    fn store_round_trip_sanitizes_and_clears_a_named_slot() {
        let path = temporary_save_path("round-trip");
        let store = SaveStore::with_path("pilot one", &path);
        let save = SaveData {
            high_score: 128,
            settings: GameSettings {
                music_volume: 2.0,
                ..Default::default()
            },
            ..Default::default()
        };

        store.save(&save).expect("save should persist");
        assert_eq!(store.slot(), "pilot-one");
        assert_eq!(
            store.load().expect("save should load").unwrap().high_score,
            128
        );
        assert_eq!(
            store
                .load()
                .expect("save should load")
                .unwrap()
                .settings
                .music_volume,
            1.0
        );
        store.clear().expect("save should clear");
        assert!(store.load().expect("empty slot should load").is_none());
    }

    #[test]
    fn newer_save_format_is_not_overwritten_by_migration() {
        let path = temporary_save_path("newer-version");
        std::fs::write(
            &path,
            r#"{"format_version":99,"settings":{"master_volume":1.0,"music_volume":1.0,"sfx_volume":1.0,"ambience_volume":1.0,"ui_volume":1.0,"post_fx_enabled":true,"screen_shake_enabled":true,"reduced_motion":false},"high_score":100,"runs_completed":1}"#,
        )
        .expect("fixture should write");

        let error = SaveStore::with_path("future", &path)
            .load()
            .expect_err("newer save must be rejected");
        assert_eq!(
            error,
            SaveError::NewerFormat {
                found: 99,
                supported: SAVE_FORMAT_VERSION,
            }
        );
        let _ = std::fs::remove_file(path);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn temporary_save_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "aurora-engine-save-{label}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("current time is after the unix epoch")
                .as_nanos()
        ))
    }
}
