//! Cross-platform, game-owned persistence.
//!
//! Aurora owns storage, slot sanitization, atomic writes, and the versioned
//! envelope.  A game owns the payload and any migration from older versions.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt, marker::PhantomData};

#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const DEFAULT_SAVE_SLOT: &str = "default";

/// A versioned payload whose schema belongs to the game that defines `T`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveEnvelope<T> {
    pub format_version: u32,
    pub payload: T,
}

impl<T> SaveEnvelope<T> {
    pub fn new(format_version: u32, payload: T) -> Self {
        Self {
            format_version,
            payload,
        }
    }
}

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
                "save format {found} is newer than supported format {supported}"
            ),
        }
    }
}
impl std::error::Error for SaveError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSource {
    Primary,
    Backup,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSave<T> {
    pub envelope: SaveEnvelope<T>,
    pub source: SaveSource,
}

/// A typed save slot. `application` prevents two games from sharing browser
/// keys or native directories even when they use the same slot name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStore<T> {
    application: String,
    slot: String,
    #[cfg(not(target_arch = "wasm32"))]
    path: PathBuf,
    marker: PhantomData<fn() -> T>,
}

impl<T> SaveStore<T> {
    pub fn new(application: impl Into<String>, slot: impl Into<String>) -> Self {
        let application = sanitize_component(application.into(), "aurora-game");
        let slot = sanitize_component(slot.into(), DEFAULT_SAVE_SLOT);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = native_save_directory()
                .join(&application)
                .join(format!("{slot}.json"));
            Self {
                application,
                slot,
                path,
                marker: PhantomData,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                application,
                slot,
                marker: PhantomData,
            }
        }
    }

    pub fn application(&self) -> &str {
        &self.application
    }
    pub fn slot(&self) -> &str {
        &self.slot
    }

    pub fn load_with_source(&self) -> Result<Option<LoadedSave<T>>, SaveError>
    where
        T: DeserializeOwned,
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let primary = read_native_save(&self.path);
            match primary {
                Ok(Some(envelope)) => Ok(Some(LoadedSave {
                    envelope,
                    source: SaveSource::Primary,
                })),
                Ok(None) => match read_native_save(&native_backup_path(&self.path)) {
                    Ok(Some(envelope)) => Ok(Some(LoadedSave {
                        envelope,
                        source: SaveSource::Backup,
                    })),
                    Ok(None) => Ok(None),
                    Err(error) => Err(error),
                },
                Err(primary_error @ SaveError::Serialization(_)) => {
                    match read_native_save(&native_backup_path(&self.path)) {
                        Ok(Some(envelope)) => Ok(Some(LoadedSave {
                            envelope,
                            source: SaveSource::Backup,
                        })),
                        Ok(None) | Err(_) => Err(primary_error),
                    }
                }
                Err(primary_error) => match read_native_save(&native_backup_path(&self.path)) {
                    Ok(Some(envelope)) => Ok(Some(LoadedSave {
                        envelope,
                        source: SaveSource::Backup,
                    })),
                    Ok(None) => Err(primary_error),
                    Err(backup_error) => Err(combine_load_errors(primary_error, backup_error)),
                },
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let storage = browser_storage()?;
            let primary_key = browser_key(&self.application, &self.slot);
            let backup_key = browser_backup_key(&self.application, &self.slot);
            let primary = storage
                .get_item(&primary_key)
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?;
            match primary {
                Some(json) => match decode(json.as_bytes()) {
                    Ok(envelope) => Ok(Some(LoadedSave {
                        envelope,
                        source: SaveSource::Primary,
                    })),
                    Err(primary_error @ SaveError::Serialization(_)) => {
                        match storage
                            .get_item(&backup_key)
                            .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?
                        {
                            Some(json) => match decode(json.as_bytes()) {
                                Ok(envelope) => Ok(Some(LoadedSave {
                                    envelope,
                                    source: SaveSource::Backup,
                                })),
                                Err(_) => Err(primary_error),
                            },
                            None => Err(primary_error),
                        }
                    }
                    Err(error) => Err(error),
                },
                None => match storage
                    .get_item(&backup_key)
                    .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?
                {
                    Some(json) => Ok(Some(LoadedSave {
                        envelope: decode(json.as_bytes())?,
                        source: SaveSource::Backup,
                    })),
                    None => Ok(None),
                },
            }
        }
    }

    pub fn load(&self) -> Result<Option<SaveEnvelope<T>>, SaveError>
    where
        T: DeserializeOwned,
    {
        self.load_with_source()
            .map(|loaded| loaded.map(|loaded| loaded.envelope))
    }

    /// Loads and lets the game migrate an older envelope. Future versions are
    /// rejected before the game sees them.
    pub fn load_with<F>(
        &self,
        supported_version: u32,
        migrate: F,
    ) -> Result<Option<SaveEnvelope<T>>, SaveError>
    where
        T: DeserializeOwned,
        F: FnOnce(SaveEnvelope<T>) -> Result<SaveEnvelope<T>, SaveError>,
    {
        let Some(save) = self.load_with_source()?.map(|loaded| loaded.envelope) else {
            return Ok(None);
        };
        if save.format_version > supported_version {
            return Err(SaveError::NewerFormat {
                found: save.format_version,
                supported: supported_version,
            });
        }
        migrate(save).map(Some)
    }

    pub fn save(&self, save: &SaveEnvelope<T>) -> Result<(), SaveError>
    where
        T: Serialize,
    {
        let bytes = serde_json::to_vec_pretty(save)
            .map_err(|error| SaveError::Serialization(error.to_string()))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            save_native_bytes(&self.path, &bytes)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let json = String::from_utf8(bytes)
                .map_err(|error| SaveError::Serialization(error.to_string()))?;
            let storage = browser_storage()?;
            let primary_key = browser_key(&self.application, &self.slot);
            let backup_key = browser_backup_key(&self.application, &self.slot);
            if let Some(previous) = storage
                .get_item(&primary_key)
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?
            {
                storage
                    .set_item(&backup_key, &previous)
                    .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?;
            }
            storage
                .set_item(&primary_key, &json)
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))
        }
    }

    pub fn clear(&self) -> Result<(), SaveError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut first_error = None;
            for path in [&self.path, &native_backup_path(&self.path)] {
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) if first_error.is_none() => {
                        first_error = Some(SaveError::Io(error.to_string()));
                    }
                    Err(_) => {}
                }
            }
            first_error.map_or(Ok(()), Err)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let storage = browser_storage()?;
            let mut first_error = None;
            for key in [
                browser_key(&self.application, &self.slot),
                browser_backup_key(&self.application, &self.slot),
            ] {
                if let Err(error) = storage.remove_item(&key) {
                    if first_error.is_none() {
                        first_error = Some(SaveError::StorageUnavailable(js_error(&error)));
                    }
                }
            }
            first_error.map_or(Ok(()), Err)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_path(
        application: impl Into<String>,
        slot: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            application: sanitize_component(application.into(), "aurora-game"),
            slot: sanitize_component(slot.into(), DEFAULT_SAVE_SLOT),
            path: path.into(),
            marker: PhantomData,
        }
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<SaveEnvelope<T>, SaveError> {
    serde_json::from_slice(bytes).map_err(|error| SaveError::Serialization(error.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
fn combine_load_errors(primary: SaveError, backup: SaveError) -> SaveError {
    match (primary, backup) {
        (SaveError::Io(primary), SaveError::Io(backup)) => {
            SaveError::Io(format!("{primary}; backup: {backup}"))
        }
        (primary, _) => primary,
    }
}

#[cfg(not(target_arch = "wasm32"))]
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[cfg(not(target_arch = "wasm32"))]
fn read_native_save<T: DeserializeOwned>(
    path: &Path,
) -> Result<Option<SaveEnvelope<T>>, SaveError> {
    match fs::read(path) {
        Ok(bytes) => decode(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SaveError::Io(error.to_string())),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_backup_path(path: &Path) -> PathBuf {
    path.with_extension("bak")
}

#[cfg(not(target_arch = "wasm32"))]
fn unique_sibling(path: &Path, suffix: &str) -> PathBuf {
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("save");
    path.with_file_name(format!("{name}.{suffix}.{}.{}", std::process::id(), id))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| SaveError::Io(error.to_string()))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(SaveError::Io(error.to_string()));
    }
    drop(file);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn copy_synced(source: &Path, destination: &Path) -> Result<(), SaveError> {
    let mut source_file =
        fs::File::open(source).map_err(|error| SaveError::Io(error.to_string()))?;
    let mut destination_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| SaveError::Io(error.to_string()))?;
    if let Err(error) =
        io::copy(&mut source_file, &mut destination_file).and_then(|_| destination_file.sync_all())
    {
        let _ = fs::remove_file(destination);
        return Err(SaveError::Io(error.to_string()));
    }
    drop(destination_file);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn replace_native_path(temporary: &Path, destination: &Path) -> Result<(), SaveError> {
    #[cfg(windows)]
    if destination.exists() {
        fs::remove_file(destination).map_err(|error| SaveError::Io(error.to_string()))?;
    }
    fs::rename(temporary, destination).map_err(|error| SaveError::Io(error.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
fn save_native_bytes(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    let parent = path
        .parent()
        .ok_or_else(|| SaveError::Io("save path has no parent directory".into()))?;
    fs::create_dir_all(parent).map_err(|error| SaveError::Io(error.to_string()))?;

    let temporary = unique_sibling(path, "tmp");
    write_synced(&temporary, bytes)?;

    let backup = native_backup_path(path);
    if path.exists() {
        let backup_temporary = unique_sibling(path, "bak-tmp");
        if let Err(error) = copy_synced(path, &backup_temporary)
            .and_then(|_| replace_native_path(&backup_temporary, &backup))
        {
            let _ = fs::remove_file(&temporary);
            let _ = fs::remove_file(&backup_temporary);
            return Err(error);
        }
    }

    if let Err(error) = replace_native_path(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn sanitize_component(value: String, fallback: &str) -> String {
    let mut normalized: String = value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '-',
        })
        .collect();
    normalized.truncate(64);
    if normalized.trim_matches('-').is_empty() {
        fallback.to_owned()
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
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aurora-engine")
}

#[cfg(target_arch = "wasm32")]
fn browser_key(application: &str, slot: &str) -> String {
    format!("aurora:{application}:{slot}")
}
#[cfg(target_arch = "wasm32")]
fn browser_backup_key(application: &str, slot: &str) -> String {
    format!("aurora:{application}:{slot}:backup")
}
#[cfg(target_arch = "wasm32")]
fn browser_storage() -> Result<web_sys::Storage, SaveError> {
    web_sys::window()
        .ok_or_else(|| SaveError::StorageUnavailable("no browser window".into()))?
        .local_storage()
        .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?
        .ok_or_else(|| SaveError::StorageUnavailable("local storage unavailable".into()))
}
#[cfg(target_arch = "wasm32")]
fn js_error(error: &wasm_bindgen::JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| "browser storage error".into())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Payload {
        score: u64,
    }

    fn test_store(name: &str) -> (SaveStore<Payload>, std::path::PathBuf) {
        let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aurora-save-{name}-{}-{id}.json",
            std::process::id()
        ));
        let store = SaveStore::<Payload>::with_path("test-game", name, &path);
        let _ = store.clear();
        (store, path)
    }
    #[test]
    fn future_versions_are_rejected_before_migration() {
        let dir = std::env::temp_dir().join("aurora-generic-save-test.json");
        let store = SaveStore::<Payload>::with_path("test-game", "pilot one", &dir);
        store
            .save(&SaveEnvelope::new(3, Payload { score: 7 }))
            .unwrap();
        assert!(matches!(
            store.load_with(2, Ok),
            Err(SaveError::NewerFormat { .. })
        ));
        store.clear().unwrap();
    }
    #[test]
    fn slots_are_namespaced_and_sanitized() {
        let store = SaveStore::<Payload>::new("A Test/Game", "pilot one");
        assert_eq!(store.application(), "A-Test-Game");
        assert_eq!(store.slot(), "pilot-one");
    }

    #[test]
    fn malformed_primary_recovers_from_backup_and_reports_source() {
        let (store, path) = test_store("recovery");
        store
            .save(&SaveEnvelope::new(1, Payload { score: 1 }))
            .unwrap();
        store
            .save(&SaveEnvelope::new(1, Payload { score: 7 }))
            .unwrap();
        std::fs::write(&path, b"{").unwrap();

        let loaded = store.load_with_source().unwrap().unwrap();
        assert_eq!(loaded.source, SaveSource::Backup);
        assert_eq!(loaded.envelope.payload, Payload { score: 1 });
        store.clear().unwrap();
    }

    #[test]
    fn future_versions_are_rejected_after_source_recovery_is_selected() {
        let (store, _) = test_store("future");
        store
            .save(&SaveEnvelope::new(9, Payload { score: 3 }))
            .unwrap();
        assert!(matches!(
            store.load_with(4, Ok),
            Err(SaveError::NewerFormat {
                found: 9,
                supported: 4,
            })
        ));
        store.clear().unwrap();
    }

    #[test]
    fn clear_removes_primary_and_backup_only_for_the_selected_slot() {
        let (store, path) = test_store("clear");
        store
            .save(&SaveEnvelope::new(1, Payload { score: 1 }))
            .unwrap();
        store
            .save(&SaveEnvelope::new(1, Payload { score: 2 }))
            .unwrap();
        assert!(path.with_extension("bak").exists());
        store.clear().unwrap();
        assert!(!path.exists());
        assert!(!path.with_extension("bak").exists());
    }
}
