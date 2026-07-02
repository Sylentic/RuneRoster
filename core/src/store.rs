//! Secret storage: one refresh token per profile, in the OS credential store.
//!
//! Wraps the `keyring` crate (Windows Credential Manager on Windows, Secret Service on
//! Linux once that platform is picked up) behind a small API keyed by profile UUID. No
//! custom crypto — this just delegates to the OS-native secret store.
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

/// Stores/retrieves a profile's Jagex refresh token in the OS credential store.
pub struct TokenStore;

impl TokenStore {
    fn entry(id: Uuid) -> Result<Entry, StoreError> {
        Ok(Entry::new(SERVICE_NAME, &id.to_string())?)
    }

    pub fn save_refresh_token(id: Uuid, refresh_token: &str) -> Result<(), StoreError> {
        Self::entry(id)?.set_password(refresh_token)?;
        Ok(())
    }

    pub fn load_refresh_token(id: Uuid) -> Result<String, StoreError> {
        Ok(Self::entry(id)?.get_password()?)
    }

    pub fn delete_refresh_token(id: Uuid) -> Result<(), StoreError> {
        Self::entry(id)?.delete_credential()?;
        Ok(())
    }
}
