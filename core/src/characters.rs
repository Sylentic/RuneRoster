//! Character/account listing for a given Jagex game session.
//!
//! `GET /game-session/v1/accounts` on `auth.jagex.com`, authenticated with the session ID
//! minted in `crate::auth::create_game_session`. Response shape (`accountId`, `displayName`)
//! confirmed by reading RuneLite/Bolt client code as a reference — this module makes the
//! request itself rather than reusing any of it.

use serde::Deserialize;

use crate::auth::AUTH_ORIGIN;

const ENDPOINT_ACCOUNTS: &str = "/game-session/v1/accounts";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Character {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CharacterError {
    #[error("network request failed: {0}")]
    Request(#[from] reqwest::Error),
}

impl CharacterError {
    /// True if this looks like an expired/invalid session (HTTP 401) rather than a
    /// transient network failure — callers should prompt the user to log in again via
    /// `auth::LoginFlow` instead of just retrying. See `crate::session::reconnect_profile`.
    pub fn is_unauthorized(&self) -> bool {
        match self {
            CharacterError::Request(e) => e.status() == Some(reqwest::StatusCode::UNAUTHORIZED),
        }
    }
}

/// Lists the characters available under a given game session.
pub async fn list_characters(
    http: &reqwest::Client,
    session_id: &str,
) -> Result<Vec<Character>, CharacterError> {
    let resp = http
        .get(format!("{AUTH_ORIGIN}{ENDPOINT_ACCOUNTS}"))
        .bearer_auth(session_id)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<Vec<Character>>().await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_character_list() {
        let json = r#"[{"accountId":"123","displayName":"Zezima"}]"#;
        let characters: Vec<Character> = serde_json::from_str(json).unwrap();
        assert_eq!(
            characters,
            vec![Character {
                account_id: "123".into(),
                display_name: "Zezima".into(),
            }]
        );
    }
}
