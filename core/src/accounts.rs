//! Multi-account profile management.
//!
//! A `Profile` is RuneRoster's own concept, not present in Bolt (which only tracks a single
//! logged-in account at a time) — this is the core addition that makes multi-accounting work.
//! Profile metadata (id + display name) is non-secret and persisted as plain JSON via
//! `ProfileRegistry`; secrets (refresh tokens) live in the OS keyring, see `crate::store`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One tracked Jagex account, launched with its own isolated RuneLite settings profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub id: Uuid,
    /// Your own label for this profile — not the Jagex account's display name.
    pub display_name: String,
}

impl Profile {
    pub fn new(display_name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            display_name: display_name.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("failed to read/write profiles file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse profiles file: {0}")]
    Parse(#[from] serde_json::Error),
}

/// The list of known profiles, persisted as plain JSON (no secrets here).
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileRegistry {
    pub profiles: Vec<Profile>,
}

impl ProfileRegistry {
    /// Loads the registry from `path`, or an empty registry if the file doesn't exist yet.
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&data)?)
    }

    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn add(&mut self, display_name: impl Into<String>) -> &Profile {
        self.profiles.push(Profile::new(display_name));
        self.profiles.last().expect("just pushed")
    }

    pub fn remove(&mut self, id: Uuid) {
        self.profiles.retain(|p| p.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_round_trip() {
        let mut registry = ProfileRegistry::default();
        let id = registry.add("Main").id;
        assert_eq!(registry.profiles.len(), 1);
        registry.remove(id);
        assert!(registry.profiles.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut registry = ProfileRegistry::default();
        registry.add("Main");
        registry.add("Ironman alt");

        let dir = std::env::temp_dir().join(format!("runeroster-test-{}", Uuid::new_v4()));
        let path = dir.join("profiles.json");
        registry.save(&path).unwrap();

        let loaded = ProfileRegistry::load(&path).unwrap();
        assert_eq!(loaded, registry);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_missing_file_returns_empty_registry() {
        let path = std::env::temp_dir().join(format!("runeroster-missing-{}", Uuid::new_v4()));
        let registry = ProfileRegistry::load(&path).unwrap();
        assert!(registry.profiles.is_empty());
    }
}
