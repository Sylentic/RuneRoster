//! Jagex OAuth2 + OIDC login flow.
//!
//! This is an original Rust implementation of the login flow used by the official Jagex
//! launcher. Nothing here is copied from Bolt's C++ source (`Adamcake/Bolt`,
//! `src/browser/window_login.cxx`) — it was read only as a reference to understand the
//! protocol. The endpoints, client ID, and scopes below are protocol facts required to
//! interoperate with Jagex's OAuth server (dictated by Jagex's API contract, not creative
//! expression), not Bolt's source code.
//!
//! ## Flow summary
//! 1. Build a PKCE verifier/challenge and a "login" authorize URL, open it in a browser.
//! 2. Jagex redirects to `redirect_uri` with `?code=...&state=...` after the user logs in.
//! 3. Exchange `code` (+ verifier) for `{id_token, access_token, refresh_token}` at the token
//!    endpoint.
//! 4. Build a *second* ("consent") authorize URL using `id_token` as a hint, so the user
//!    isn't prompted to log in twice. Jagex redirects to `http://localhost` with
//!    `#code=...&id_token=...` in the URL fragment.
//! 5. POST that second `id_token` to the game-session endpoint to mint a `session_id`,
//!    which is then used as a bearer token for the character list and handed to RuneLite.
//!
//! ## Known gap (unverified against the live server)
//! Step 2's redirect target in the official flow is a real Jagex-owned page
//! (`secure.runescape.com/m=weblogin/launcher-redirect`). Bolt can treat that as a pure
//! signal because it intercepts browser navigation client-side (it embeds a Chromium
//! instance). Without an embedded browser, capturing that redirect from the user's *system*
//! browser needs to be verified against the real server: either a loopback HTTP listener
//! (plausible for step 4, whose redirect target is `http://localhost`) or a registered
//! custom URL scheme for step 2. This module implements the request-building and
//! token-exchange logic; the browser-launch + redirect-capture UX is deliberately left as a
//! separate, not-yet-verified concern (see `core::launcher` and future `LoginFlow` work).

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Jagex's account/OAuth origin.
pub const ACCOUNT_ORIGIN: &str = "https://account.jagex.com";
/// Jagex's game-session origin (character list, session creation) — see `crate::characters`.
pub const AUTH_ORIGIN: &str = "https://auth.jagex.com";

const ENDPOINT_AUTH: &str = "/oauth2/auth";
const ENDPOINT_TOKEN: &str = "/oauth2/token";
const ENDPOINT_SESSION: &str = "/game-session/v1/sessions";

/// OAuth client ID used by the official desktop launcher.
pub const CLIENT_ID: &str = "com_jagex_auth_desktop_launcher";

/// Scopes requested in the first (login) authorization request.
const LOGIN_SCOPES: &str = "openid offline gamesso.token.create user.profile.read user.entitlement.read user.game.read user.sku.read user.voucher.redeem";
/// Scopes requested in the second (consent) authorization request.
const CONSENT_SCOPES: &str = "openid offline";

const VERIFIER_CHARS: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._";
const VERIFIER_LENGTH: usize = 96;
const STATE_LENGTH: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("network request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("invalid or malformed id_token")]
    InvalidIdToken,
}

/// A PKCE verifier/challenge pair. Generate one fresh per login attempt.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        let verifier = random_string(VERIFIER_LENGTH, VERIFIER_CHARS);
        let challenge = code_challenge_s256(&verifier);
        Self { verifier, challenge }
    }
}

fn code_challenge_s256(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, digest)
}

fn random_string(len: usize, alphabet: &[u8]) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| alphabet[rng.gen_range(0..alphabet.len())] as char)
        .collect()
}

/// Generates a random state/nonce string used to correlate an authorize request with its
/// redirect response.
pub fn generate_state() -> String {
    random_string(STATE_LENGTH, b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz")
}

/// Builds the first-leg ("login") authorize URL.
pub fn build_login_url(redirect_uri: &str, state: &str, pkce: &Pkce) -> String {
    format!(
        "{ACCOUNT_ORIGIN}{ENDPOINT_AUTH}?auth_method=&login_type=&flow=launcher&response_type=code\
         &client_id={CLIENT_ID}&code_challenge_method=S256&prompt=login&scope={scope}\
         &redirect_uri={redirect}&code_challenge={challenge}&state={state}",
        scope = urlencoding::encode(LOGIN_SCOPES),
        redirect = urlencoding::encode(redirect_uri),
        challenge = pkce.challenge,
    )
}

/// Builds the second-leg ("consent") authorize URL, using the `id_token` from the first
/// exchange as a hint so the user isn't prompted to log in twice.
pub fn build_consent_url(id_token: &str, state: &str, nonce: &str) -> String {
    format!(
        "{ACCOUNT_ORIGIN}{ENDPOINT_AUTH}?prompt=consent&redirect_uri=http%3A%2F%2Flocalhost\
         &response_type=id_token+code&client_id={CLIENT_ID}&scope={scope}\
         &id_token_hint={id_token}&state={state}&nonce={nonce}",
        scope = urlencoding::encode(CONSENT_SCOPES),
    )
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

/// Exchanges a first-leg authorization `code` for tokens.
pub async fn exchange_code(
    http: &reqwest::Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, AuthError> {
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect_uri),
    ];
    let resp = http
        .post(format!("{ACCOUNT_ORIGIN}{ENDPOINT_TOKEN}"))
        .form(&params)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<TokenResponse>().await?)
}

/// Uses a stored `refresh_token` to mint a fresh token set without a full interactive login.
pub async fn refresh_tokens(
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<TokenResponse, AuthError> {
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
    ];
    let resp = http
        .post(format!("{ACCOUNT_ORIGIN}{ENDPOINT_TOKEN}"))
        .form(&params)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<TokenResponse>().await?)
}

/// Decodes the (unverified) claims body of a JWT, e.g. to read `sub` out of an `id_token`.
/// This does not verify the signature — it's only used to read claims from a token received
/// directly from Jagex's own token endpoint over TLS, not from an untrusted third party.
pub fn decode_id_token_claims(token: &str) -> Result<serde_json::Value, AuthError> {
    let mut parts = token.split('.');
    let _header = parts.next().ok_or(AuthError::InvalidIdToken)?;
    let body = parts.next().ok_or(AuthError::InvalidIdToken)?;
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, body)
        .map_err(|_| AuthError::InvalidIdToken)?;
    serde_json::from_slice(&decoded).map_err(|_| AuthError::InvalidIdToken)
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    #[serde(rename = "sessionId")]
    session_id: String,
}

/// Exchanges the consent-step `id_token` for a game session ID — used as a bearer token
/// against `auth.jagex.com` (see `crate::characters`) and passed to RuneLite at launch.
pub async fn create_game_session(
    http: &reqwest::Client,
    id_token: &str,
) -> Result<String, AuthError> {
    let body = serde_json::json!({ "idToken": id_token });
    let resp = http
        .post(format!("{AUTH_ORIGIN}{ENDPOINT_SESSION}"))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    let parsed: SessionResponse = resp.json().await?;
    Ok(parsed.session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_uses_allowed_charset_and_length() {
        let pkce = Pkce::generate();
        assert_eq!(pkce.verifier.len(), VERIFIER_LENGTH);
        assert!(pkce
            .verifier
            .bytes()
            .all(|b| VERIFIER_CHARS.contains(&b)));
        // S256 challenge is a 32-byte SHA-256 digest, base64url-encoded without padding.
        assert_eq!(pkce.challenge.len(), 43);
        assert!(!pkce.challenge.contains('='));
    }

    #[test]
    fn state_is_alphabetic_and_expected_length() {
        let state = generate_state();
        assert_eq!(state.len(), STATE_LENGTH);
        assert!(state.chars().all(|c| c.is_ascii_alphabetic()));
    }

    #[test]
    fn login_url_contains_expected_fixed_params() {
        let pkce = Pkce::generate();
        let state = generate_state();
        let url = build_login_url("https://example.test/callback", &state, &pkce);
        assert!(url.starts_with(ACCOUNT_ORIGIN));
        assert!(url.contains("client_id=com_jagex_auth_desktop_launcher"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("code_challenge={}", pkce.challenge)));
        assert!(url.contains(&format!("state={state}")));
    }

    #[test]
    fn decodes_id_token_claims() {
        // header.payload.signature, where payload = base64url({"sub":"abc123"})
        let payload = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            br#"{"sub":"abc123"}"#,
        );
        let token = format!("header.{payload}.signature");
        let claims = decode_id_token_claims(&token).unwrap();
        assert_eq!(claims["sub"], "abc123");
    }

    #[test]
    fn rejects_malformed_id_token() {
        assert!(decode_id_token_claims("not-a-jwt").is_err());
    }
}
