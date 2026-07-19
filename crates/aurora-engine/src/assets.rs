//! Stable asset keys and a small manifest for game-facing asset references.

use std::collections::BTreeMap;
use std::fmt;

/// A validated, stable key such as `characters.runner` or `audio.collect`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetKey(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetKeyError {
    Empty,
    InvalidCharacter(char),
    EmptySegment,
}

impl fmt::Display for AssetKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "asset key cannot be empty"),
            Self::InvalidCharacter(ch) => write!(f, "invalid asset key character: {ch}"),
            Self::EmptySegment => write!(f, "asset key contains an empty segment"),
        }
    }
}

impl std::error::Error for AssetKeyError {}

impl AssetKey {
    pub fn new(value: impl AsRef<str>) -> Result<Self, AssetKeyError> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(AssetKeyError::Empty);
        }
        if value.split('.').any(str::is_empty) {
            return Err(AssetKeyError::EmptySegment);
        }
        for ch in value.chars() {
            if !(ch.is_ascii_lowercase()
                || ch.is_ascii_digit()
                || ch == '.'
                || ch == '_'
                || ch == '-')
            {
                return Err(AssetKeyError::InvalidCharacter(ch));
            }
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Texture,
    SpriteAtlas,
    Audio,
    Font,
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEntry {
    pub kind: AssetKind,
    /// Path relative to the game's asset root.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetManifestError {
    DuplicateKey(AssetKey),
    UnsafePath(String),
}

impl fmt::Display for AssetManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateKey(key) => write!(f, "asset key already registered: {key}"),
            Self::UnsafePath(path) => write!(
                f,
                "asset path must be relative and cannot escape the asset root: {path}"
            ),
        }
    }
}

impl std::error::Error for AssetManifestError {}

/// Deterministic manifest keyed by gameplay-facing names rather than filenames.
#[derive(Debug, Default, Clone)]
pub struct AssetManifest {
    entries: BTreeMap<AssetKey, AssetEntry>,
}

impl AssetManifest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        key: AssetKey,
        kind: AssetKind,
        path: impl Into<String>,
    ) -> Result<(), AssetManifestError> {
        let path = path.into();
        if !is_safe_relative_path(&path) {
            return Err(AssetManifestError::UnsafePath(path));
        }
        if self.entries.contains_key(&key) {
            return Err(AssetManifestError::DuplicateKey(key));
        }
        self.entries.insert(key, AssetEntry { kind, path });
        Ok(())
    }

    pub fn get(&self, key: &AssetKey) -> Option<&AssetEntry> {
        self.entries.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&AssetKey, &AssetEntry)> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path
            .split(['/', '\\'])
            .any(|segment| segment == ".." || segment.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_uses_stable_keys_and_rejects_escaping_paths() {
        let key = AssetKey::new("characters.runner").unwrap();
        let mut manifest = AssetManifest::new();
        manifest
            .insert(key.clone(), AssetKind::SpriteAtlas, "sprites/runner.png")
            .unwrap();
        assert_eq!(manifest.get(&key).unwrap().kind, AssetKind::SpriteAtlas);
        assert!(matches!(
            manifest.insert(
                AssetKey::new("characters.bad").unwrap(),
                AssetKind::Texture,
                "../bad.png"
            ),
            Err(AssetManifestError::UnsafePath(_))
        ));
    }
}
