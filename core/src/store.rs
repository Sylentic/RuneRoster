//! Secret storage: one game `session_id` per profile, in the OS credential store.
//!
//! **Not an OAuth refresh token** — Jagex's game-session API has no `grant_type=refresh_token`
//! call; instead the game `session_id` itself (from `auth::create_game_session`) is persisted
//! and reused directly across app restarts, only redoing the full `auth::LoginFlow` when that
//! `session_id` eventually gets rejected (HTTP 401 — see
//! `characters::CharacterError::is_unauthorized` and `session::reconnect_profile`).
//! `session_id` is also exactly what gets handed to RuneLite as `JX_SESSION_ID` at launch.
//!
//! Wraps the `keyring` crate (Windows Credential Manager on Windows, Secret Service on
//! Linux once that platform is picked up) behind a small API keyed by profile UUID. No
//! custom crypto — this just delegates to the OS-native secret store. Requires the
//! `windows-native` cargo feature to actually reach the real Windows Credential Manager
//! instead of silently falling back to an in-memory mock — see repo memory notes.
//!
//! Keyed by `Uuid` rather than `&Profile` so an entry can still be cleaned up (deleted)
//! after its profile has already been removed from `ProfileRegistry` — see
//! `crate::accounts::remove_profile`.

use keyring::Entry;
use uuid::Uuid;

const SERVICE_NAME: &str = "RuneRoster";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("keyring error: {0}")]
    Keyring(#[from] keyring::Error),
}

/// Stores/retrieves a profile's game `session_id` in the OS credential store.
pub struct TokenStore;

impl TokenStore {
    fn entry(id: Uuid) -> Result<Entry, StoreError> {
        Ok(Entry::new(SERVICE_NAME, &id.to_string())?)
    }

    pub fn save_session_id(id: Uuid, session_id: &str) -> Result<(), StoreError> {
        Self::entry(id)?.set_password(session_id)?;
        Ok(())
    }

    pub fn load_session_id(id: Uuid) -> Result<String, StoreError> {
        Ok(Self::entry(id)?.get_password()?)
    }

    pub fn delete_session_id(id: Uuid) -> Result<(), StoreError> {
        Self::entry(id)?.delete_credential()?;
        Ok(())
    }
}
