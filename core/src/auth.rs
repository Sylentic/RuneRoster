//! Jagex OAuth2 + OIDC login flow.
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
//! ## Login flow, confirmed against the live server
//! Step 2's redirect target in the official flow is a real Jagex-owned page
//! (`secure.runescape.com/m=weblogin/launcher-redirect`). Bolt treats that as a pure signal
//! because it intercepts browser navigation client-side (it embeds a Chromium instance).
//! Tested two alternatives against the live server instead:
//! - Substituting a loopback `redirect_uri` (`http://127.0.0.1:PORT/...`): **rejected** by
//!   Jagex (`invalid_request` — redirect_uri not pre-registered for this client ID). No
//!   custom URL scheme registration exists in Bolt's source either, so that's not an option.
//! - Using the *real* registered redirect_uri in an ordinary system browser: **works** — the
//!   resulting page's address bar plainly contains `?code=...&state=...`, confirmed live.
//!   So a manual copy-paste of the resulting URL is a working fallback that needs no
//!   embedded browser at all (see `parse_redirect_url` below, and the `login_test` example).
//!
//! Step 4 (the consent leg, redirecting to `http://localhost`) is also confirmed live now:
//! its `code`/`id_token`/`state` arrive in the URL **fragment**, not the query string
//! (fragments never reach a server), so it's a second manual paste, and it requires a
//! *different* client ID (`CONSENT_CLIENT_ID`) than the login leg — see that constant's doc
//! comment. `LoginFlow` (below) consolidates the whole verified dance into one API.

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Jagex's account/OAuth origin.
pub const ACCOUNT_ORIGIN: &str = "https://account.jagex.com";
/// Jagex's game-session origin (character list, session creation) — see `crate::characters`.
pub const AUTH_ORIGIN: &str = "https://auth.jagex.com";

const ENDPOINT_AUTH: &str = "/oauth2/auth";
const ENDPOINT_TOKEN: &str = "/oauth2/token";
const ENDPOINT_SESSION: &str = "/game-session/v1/sessions";

/// The only redirect_uri registered for `CLIENT_ID`'s login leg (confirmed live — see
/// module docs). Not a loopback address: this is a real Jagex-owned page, so capturing its
/// redirect requires the user to paste the resulting URL back in (see `LoginFlow`).
pub const LOGIN_REDIRECT_URI: &str = "https://secure.runescape.com/m=weblogin/launcher-redirect";

/// OAuth client ID used by the official desktop launcher for the first (login) leg.
pub const CLIENT_ID: &str = "com_jagex_auth_desktop_launcher";

/// A *second*, distinct OAuth client ID used only for the consent leg (confirmed against
/// the live server: using `CLIENT_ID` there gets rejected with `invalid_request` since it's
/// not the one registered with `http://localhost` as a redirect_uri). Bolt hardcodes this
/// as "the PRODUCTION value" with separate DEVELOPMENT/STAGING variants noted in a comment
/// — this is the registered identifier itself (a protocol fact needed to interoperate),
/// not creative expression.
const CONSENT_CLIENT_ID: &str = "1fddee4e-b100-4f4e-b2b0-097f9088f9d2";

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
    #[error("redirect URL is missing a code or state parameter")]
    InvalidRedirect,
    #[error("redirect state does not match the state sent in the authorize request (possible CSRF)")]
    StateMismatch,
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
/// exchange as a hint so the user isn't prompted to log in twice. Uses `CONSENT_CLIENT_ID`,
/// not `CLIENT_ID` — see that constant's doc comment for why.
pub fn build_consent_url(id_token: &str, state: &str, nonce: &str) -> String {
    format!(
        "{ACCOUNT_ORIGIN}{ENDPOINT_AUTH}?prompt=consent&redirect_uri=http%3A%2F%2Flocalhost\
         &response_type=id_token+code&client_id={CONSENT_CLIENT_ID}&scope={scope}\
         &id_token_hint={id_token}&state={state}&nonce={nonce}",
        scope = urlencoding::encode(CONSENT_SCOPES),
    )
}

/// The `code` and `id_token` extracted from a second-leg (consent) redirect URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRedirect {
    pub code: String,
    pub id_token: String,
}

/// Parses `code` and `id_token` out of a second-leg (consent) redirect URL, verifying its
/// `state`. Unlike the first leg, these values arrive in the URL **fragment** (after `#`),
/// per the `response_type=id_token+code` request — fragments are never sent to a server, so
/// this only works when the user pastes the resulting URL back in (the fragment stays
/// visible in the browser's address bar even though a local HTTP listener would never
/// receive it over the network).
pub fn parse_consent_redirect(
    url: &str,
    expected_state: &str,
) -> Result<ConsentRedirect, AuthError> {
    let fragment = url
        .split_once('#')
        .map(|(_, f)| f)
        .ok_or(AuthError::InvalidRedirect)?;
    let params: std::collections::HashMap<&str, String> = fragment
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| {
            (
                k,
                urlencoding::decode(v)
                    .map(|c| c.into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();

    let state = params.get("state").ok_or(AuthError::InvalidRedirect)?;
    if state != expected_state {
        return Err(AuthError::StateMismatch);
    }
    let code = params.get("code").ok_or(AuthError::InvalidRedirect)?.clone();
    let id_token = params
        .get("id_token")
        .ok_or(AuthError::InvalidRedirect)?
        .clone();
    Ok(ConsentRedirect { code, id_token })
}

/// The `code` extracted from a first-leg redirect URL (whether captured automatically or
/// pasted in manually — see `login_test` example for why manual paste is the confirmed
/// working approach for this particular leg).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectCode {
    pub code: String,
}

/// Parses `code` out of a first-leg redirect URL (e.g. one the user pasted in after
/// logging in) and verifies its `state` matches the one generated for this login attempt,
/// guarding against CSRF / mixed-up login attempts.
pub fn parse_redirect_url(url: &str, expected_state: &str) -> Result<RedirectCode, AuthError> {
    let query = url
        .split_once('?')
        .map(|(_, q)| q)
        .ok_or(AuthError::InvalidRedirect)?;
    let params: std::collections::HashMap<&str, String> = query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| {
            (
                k,
                urlencoding::decode(v)
                    .map(|c| c.into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();

    let state = params.get("state").ok_or(AuthError::InvalidRedirect)?;
    if state != expected_state {
        return Err(AuthError::StateMismatch);
    }
    let code = params.get("code").ok_or(AuthError::InvalidRedirect)?.clone();
    Ok(RedirectCode { code })
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

/// Errors from driving a `LoginFlow` end to end — wraps both OAuth errors and character
/// listing errors (a different error type in `crate::characters`) behind one type so
/// callers only need to handle one `Result`.
#[derive(Debug, thiserror::Error)]
pub enum LoginFlowError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Character(#[from] crate::characters::CharacterError),
    #[error("this method isn't valid for the login flow's current step")]
    WrongStep,
}

/// Consolidates the two-leg login dance (verified end-to-end against the live Jagex server
/// — see module docs and `core/examples/login_test.rs`) into one reusable state machine.
///
/// Each step is driven by the caller (e.g. the UI) pasting in a URL the user copied from
/// their browser after logging in / consenting; there's no way to capture either redirect
/// automatically (the login leg targets a real Jagex-owned page, and the consent leg's
/// values arrive in a URL fragment that's never sent to a server — see module docs).
///
/// ```ignore
/// let (flow, login_url) = LoginFlow::start();
/// // open login_url in the user's browser, get the redirect URL back from them...
/// let (flow, consent_url) = flow.submit_login_redirect(&http, &login_redirect_url).await?;
/// // open consent_url, get the second redirect URL back from them...
/// let outcome = flow.submit_consent_redirect(&http, &consent_redirect_url).await?;
/// // outcome.characters -> let the user pick one; outcome.refresh_token -> persist it
/// ```
pub enum LoginFlow {
    /// Waiting for the user to log in and paste back the first redirect URL.
    AwaitingLogin { pkce: Pkce, state: String },
    /// Waiting for the user to consent and paste back the second redirect URL.
    AwaitingConsent {
        refresh_token: String,
        consent_state: String,
    },
}

impl std::fmt::Debug for LoginFlow {
    /// Deliberately redacts secrets — `refresh_token` is a live credential, and printing it
    /// via `{:?}` by accident (e.g. in a log line) would be the same mistake this codebase
    /// has already had to fix once in the test harness.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginFlow::AwaitingLogin { .. } => write!(f, "LoginFlow::AwaitingLogin"),
            LoginFlow::AwaitingConsent { .. } => write!(f, "LoginFlow::AwaitingConsent(..)"),
        }
    }
}

/// The result of a completed `LoginFlow`: a refresh token to persist for this profile (see
/// `crate::store::TokenStore`), plus the character list to show the user so they can pick
/// one to launch.
pub struct LoginOutcome {
    pub refresh_token: String,
    pub session_id: String,
    pub characters: Vec<crate::characters::Character>,
}

impl LoginFlow {
    /// Starts a new login attempt. Returns the initial flow state plus the URL to open in
    /// the user's browser.
    pub fn start() -> (LoginFlow, String) {
        let pkce = Pkce::generate();
        let state = generate_state();
        let url = build_login_url(LOGIN_REDIRECT_URI, &state, &pkce);
        (LoginFlow::AwaitingLogin { pkce, state }, url)
    }

    /// Call once the user has logged in and pasted back the URL from the first redirect.
    /// On success, returns the next flow state plus the consent URL to open next.
    pub async fn submit_login_redirect(
        self,
        http: &reqwest::Client,
        redirect_url: &str,
    ) -> Result<(LoginFlow, String), LoginFlowError> {
        let (pkce, state) = match self {
            LoginFlow::AwaitingLogin { pkce, state } => (pkce, state),
            LoginFlow::AwaitingConsent { .. } => return Err(LoginFlowError::WrongStep),
        };
        let redirect = parse_redirect_url(redirect_url, &state)?;
        let tokens = exchange_code(http, &redirect.code, &pkce.verifier, LOGIN_REDIRECT_URI).await?;
        let consent_state = generate_state();
        let nonce = generate_state();
        let consent_url = build_consent_url(&tokens.id_token, &consent_state, &nonce);
        Ok((
            LoginFlow::AwaitingConsent {
                refresh_token: tokens.refresh_token,
                consent_state,
            },
            consent_url,
        ))
    }

    /// Call once the user has consented and pasted back the URL from the second redirect.
    /// Completes the flow: mints a real game session and lists characters.
    pub async fn submit_consent_redirect(
        self,
        http: &reqwest::Client,
        redirect_url: &str,
    ) -> Result<LoginOutcome, LoginFlowError> {
        let (refresh_token, consent_state) = match self {
            LoginFlow::AwaitingConsent {
                refresh_token,
                consent_state,
            } => (refresh_token, consent_state),
            LoginFlow::AwaitingLogin { .. } => return Err(LoginFlowError::WrongStep),
        };
        let consent = parse_consent_redirect(redirect_url, &consent_state)?;
        let session_id = create_game_session(http, &consent.id_token).await?;
        let characters = crate::characters::list_characters(http, &session_id).await?;
        Ok(LoginOutcome {
            refresh_token,
            session_id,
            characters,
        })
    }
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
    fn parses_real_redirect_url_shape() {
        // Shape confirmed against the live Jagex server (see auth.rs module docs).
        let url = "https://secure.runescape.com/m=weblogin/launcher-redirect?code=abc.def&scope=offline+openid&state=STATE123";
        let parsed = parse_redirect_url(url, "STATE123").unwrap();
        assert_eq!(parsed.code, "abc.def");
    }

    #[test]
    fn rejects_redirect_url_with_mismatched_state() {
        let url = "https://example.test/callback?code=abc&state=wrong";
        assert!(matches!(
            parse_redirect_url(url, "expected"),
            Err(AuthError::StateMismatch)
        ));
    }

    #[test]
    fn rejects_redirect_url_missing_params() {
        let url = "https://example.test/callback?foo=bar";
        assert!(matches!(
            parse_redirect_url(url, "expected"),
            Err(AuthError::InvalidRedirect)
        ));
    }

    #[test]
    fn parses_consent_redirect_fragment() {
        let url = "http://localhost/#code=abc.def&id_token=header.payload.sig&state=STATE123";
        let parsed = parse_consent_redirect(url, "STATE123").unwrap();
        assert_eq!(parsed.code, "abc.def");
        assert_eq!(parsed.id_token, "header.payload.sig");
    }

    #[test]
    fn rejects_consent_redirect_with_mismatched_state() {
        let url = "http://localhost/#code=abc&id_token=x&state=wrong";
        assert!(matches!(
            parse_consent_redirect(url, "expected"),
            Err(AuthError::StateMismatch)
        ));
    }

    #[test]
    fn rejects_consent_redirect_without_fragment() {
        let url = "http://localhost/?code=abc&id_token=x&state=expected";
        assert!(matches!(
            parse_consent_redirect(url, "expected"),
            Err(AuthError::InvalidRedirect)
        ));
    }

    #[tokio::test]
    async fn login_flow_start_yields_awaiting_login_state() {
        let (flow, url) = LoginFlow::start();
        assert!(matches!(flow, LoginFlow::AwaitingLogin { .. }));
        assert!(url.starts_with(ACCOUNT_ORIGIN));
        assert!(format!("{flow:?}").contains("AwaitingLogin"));
    }

    #[tokio::test]
    async fn login_flow_rejects_consent_step_out_of_order() {
        let (flow, _) = LoginFlow::start();
        let http = reqwest::Client::new();
        let result = flow.submit_consent_redirect(&http, "http://localhost/#code=x").await;
        assert!(matches!(result, Err(LoginFlowError::WrongStep)));
    }

    #[tokio::test]
    async fn login_flow_rejects_login_step_when_awaiting_consent() {
        let flow = LoginFlow::AwaitingConsent {
            refresh_token: "irrelevant".into(),
            consent_state: "irrelevant".into(),
        };
        let http = reqwest::Client::new();
        let result = flow
            .submit_login_redirect(&http, "https://example.test/callback?code=x&state=y")
            .await;
        assert!(matches!(result, Err(LoginFlowError::WrongStep)));
    }

    #[tokio::test]
    async fn login_flow_surfaces_state_mismatch_before_any_network_call() {
        let (flow, _) = LoginFlow::start();
        let http = reqwest::Client::new();
        // Wrong state — should fail parsing before ever reaching the network.
        let result = flow
            .submit_login_redirect(&http, "https://example.test/callback?code=x&state=wrong")
            .await;
        assert!(matches!(
            result,
            Err(LoginFlowError::Auth(AuthError::StateMismatch))
        ));
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
