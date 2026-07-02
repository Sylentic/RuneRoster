//! Secret storage: one refresh token per profile, in the OS credential store.
//!
//! Wraps the `keyring` crate (Windows Credential Manager on Windows, Secret Service on
//! Linux once that platform is picked up) behind a small API keyed by profile UUID. No
//! custom crypto — this just delegates to the OS-native secret store.

use keyring::Entry;

use crate::accounts::Profile;

const SERVICE_NAME: &str = "RuneRoster";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("keyring error: {0}")]
    Keyring(#[from] keyring::Error),
}

/// Stores/retrieves a profile's Jagex refresh token in the OS credential store.
pub struct TokenStore;

impl TokenStore {
    fn entry(profile: &Profile) -> Result<Entry, StoreError> {
        Ok(Entry::new(SERVICE_NAME, &profile.id.to_string())?)
    }

    pub fn save_refresh_token(profile: &Profile, refresh_token: &str) -> Result<(), StoreError> {
        Self::entry(profile)?.set_password(refresh_token)?;
        Ok(())
    }

    pub fn load_refresh_token(profile: &Profile) -> Result<String, StoreError> {
        Ok(Self::entry(profile)?.get_password()?)
    }

    pub fn delete_refresh_token(profile: &Profile) -> Result<(), StoreError> {
        Self::entry(profile)?.delete_credential()?;
        Ok(())
    }
}
