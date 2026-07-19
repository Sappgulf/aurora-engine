//! Cross-platform, game-owned persistence.
//!
//! Aurora owns storage, slot sanitization, atomic writes, and the versioned
//! envelope.  A game owns the payload and any migration from older versions.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt, marker::PhantomData};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs, path::PathBuf};

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

    pub fn load(&self) -> Result<Option<SaveEnvelope<T>>, SaveError>
    where
        T: DeserializeOwned,
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match fs::read(&self.path) {
                Ok(bytes) => decode(&bytes).map(Some),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(SaveError::Io(error.to_string())),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let storage = browser_storage()?;
            match storage
                .get_item(&browser_key(&self.application, &self.slot))
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))?
            {
                Some(json) => decode(json.as_bytes()).map(Some),
                None => Ok(None),
            }
        }
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
        let Some(save) = self.load()? else {
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
            let parent = self
                .path
                .parent()
                .ok_or_else(|| SaveError::Io("save path has no parent directory".into()))?;
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
                .set_item(&browser_key(&self.application, &self.slot), &json)
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))
        }
    }

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
                .remove_item(&browser_key(&self.application, &self.slot))
                .map_err(|error| SaveError::StorageUnavailable(js_error(&error)))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Payload {
        score: u64,
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
}
