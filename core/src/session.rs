//! Mints the credentials needed to hand off to RuneLite for a specific character launch.

use uuid::Uuid;

use crate::characters::{list_characters, Character, CharacterError};
use crate::store::{StoreError, TokenStore};

/// Everything RuneLite needs to start directly logged into a specific character, via its
/// `JX_SESSION_ID` / `JX_CHARACTER_ID` / `JX_DISPLAY_NAME` environment-variable contract
/// (RuneLite's own documented mechanism for Jagex-account launches — see `crate::launcher`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSession {
    pub session_id: String,
    pub account_id: String,
    pub display_name: String,
}

impl LaunchSession {
    pub fn for_character(session_id: impl Into<String>, character: &Character) -> Self {
        Self {
            session_id: session_id.into(),
            account_id: character.account_id.clone(),
            display_name: character.display_name.clone(),
        }
    }
}

/// The result of successfully reconnecting to a profile: its still-valid `session_id`
/// (already stored — this is just confirming it still works) plus a fresh character list.
pub struct ProfileSession {
    pub session_id: String,
    pub characters: Vec<Character>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReconnectError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Character(#[from] CharacterError),
    /// The stored `session_id` was rejected (HTTP 401) — it's expired or was invalidated
    /// (e.g. via "End Sessions" on runescape.com). The caller needs to run `auth::LoginFlow`
    /// again for this profile and persist the new `session_id` it produces.
    #[error("this profile's session has expired — log in again to refresh it")]
    ReauthRequired,
}

/// Loads a profile's persisted `session_id` and confirms it's still valid by refreshing its
/// character list. This is what "launching" a profile should do first — the approach (see
/// `crate::store` docs) is to reuse a persisted session_id across restarts rather than
/// running the full login flow every time, only redoing it once the session actually expires.
pub async fn reconnect_profile(
    http: &reqwest::Client,
    profile_id: Uuid,
) -> Result<ProfileSession, ReconnectError> {
    let session_id = TokenStore::load_session_id(profile_id)?;
    match list_characters(http, &session_id).await {
        Ok(characters) => Ok(ProfileSession {
            session_id,
            characters,
        }),
        Err(e) if e.is_unauthorized() => Err(ReconnectError::ReauthRequired),
        Err(e) => Err(e.into()),
    }
}

