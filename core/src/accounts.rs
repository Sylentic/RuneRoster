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

use crate::store::{StoreError, TokenStore};

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

#[derive(Debug, thiserror::Error)]
pub enum AddProfileError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Completes account setup after a `crate::auth::LoginFlow` finishes: creates a new
/// `Profile`, persists its game `session_id` to the OS keyring (not an OAuth refresh token
/// — see `crate::store` docs for why), adds it to `registry`, and saves `registry` to
/// `registry_path`.
///
/// Order matters for failure safety: the keyring write happens *before* the profile is
/// added to the in-memory registry, so if it fails, nothing else has been touched. If the
/// registry file write fails after that, both the in-memory addition and the keyring entry
/// are rolled back, so `registry` never drifts from what's actually persisted on disk.
pub fn add_profile_from_login(
    registry: &mut ProfileRegistry,
    registry_path: &Path,
    display_name: impl Into<String>,
    session_id: &str,
) -> Result<Profile, AddProfileError> {
    let profile = Profile::new(display_name);

    TokenStore::save_session_id(profile.id, session_id)?;

    registry.profiles.push(profile.clone());
    if let Err(e) = registry.save(registry_path) {
        registry.profiles.retain(|p| p.id != profile.id);
        let _ = TokenStore::delete_session_id(profile.id);
        return Err(e.into());
    }

    Ok(profile)
}

/// Removes a profile: drops it from `registry` (saving to `registry_path`), and best-effort
/// deletes its keyring entry. The registry is the primary source of truth for "does this
/// profile exist" — a keyring deletion failure (e.g. entry already gone) doesn't fail the
/// whole operation, since the profile is already gone from the registry at that point.
pub fn remove_profile(
    registry: &mut ProfileRegistry,
    registry_path: &Path,
    id: Uuid,
) -> Result<(), RegistryError> {
    registry.remove(id);
    registry.save(registry_path)?;
    let _ = TokenStore::delete_session_id(id);
    Ok(())
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

    /// Exercises the real OS keyring (Windows Credential Manager) end-to-end, using a
    /// throwaway UUID-named entry that's cleaned up regardless of outcome.
    #[test]
    fn add_profile_from_login_persists_registry_and_keyring_entry() {
        let mut registry = ProfileRegistry::default();
        let dir = std::env::temp_dir().join(format!("runeroster-test-{}", Uuid::new_v4()));
        let path = dir.join("profiles.json");

        let profile =
            add_profile_from_login(&mut registry, &path, "Main", "fake-session-id").unwrap();

        assert_eq!(registry.profiles.len(), 1);
        assert_eq!(registry.profiles[0].id, profile.id);

        let loaded = ProfileRegistry::load(&path).unwrap();
        assert_eq!(loaded, registry);

        let stored = TokenStore::load_session_id(profile.id).unwrap();
        assert_eq!(stored, "fake-session-id");

        remove_profile(&mut registry, &path, profile.id).unwrap();
        assert!(registry.profiles.is_empty());
        assert!(TokenStore::load_session_id(profile.id).is_err());

        fs::remove_dir_all(&dir).ok();
    }
}
