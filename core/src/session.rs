//! Mints the credentials needed to hand off to RuneLite for a specific character launch.

use crate::characters::Character;

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
