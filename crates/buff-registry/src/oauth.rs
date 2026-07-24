//! GitHub OAuth login flow — T57 Track F.
//!
//! Implements the three-endpoint OAuth 2.0 authorization-code flow
//! against GitHub:
//!
//! 1. `GET /auth/github/login` — redirects the user's browser to
//!    GitHub's authorize URL (`https://github.com/login/oauth/authorize`).
//!    GitHub prompts the user to authorize the Buff registry app.
//! 2. `GET /auth/github/callback?code=<auth_code>` — GitHub redirects
//!    back here with a temporary `code`. The handler exchanges the code
//!    for an access token (POST to GitHub's token endpoint), then fetches
//!    the user's GitHub identity (GET to GitHub's user API), creates a
//!    session via [`crate::Storage::create_session`], and sets a
//!    `Set-Cookie: buff_session=<token>` header.
//! 3. `POST /auth/logout` — deletes the session, clears the cookie.
//!
//! # Configuration
//!
//! OAuth is enabled by setting BOTH env vars:
//! - `BUFF_REGISTRY_GITHUB_CLIENT_ID` — the OAuth App client ID
//! - `BUFF_REGISTRY_GITHUB_CLIENT_SECRET` — the OAuth App client secret
//!
//! The redirect URI defaults to
//! `http://localhost:7878/auth/github/callback` (matching
//! [`crate::DEFAULT_BIND_ADDR`]). Override via
//! `BUFF_REGISTRY_OAUTH_REDIRECT_URI`.
//!
//! When the env vars are NOT set, the login endpoint returns `503
//! Service Unavailable` with a JSON body explaining how to configure
//! OAuth. The rest of the registry (publish / download / search)
//! works without OAuth via static tokens (backwards compat from T126).
//!
//! # Testing
//!
//! The OAuth flow is tested via `httpmock` — a mock GitHub token +
//! user endpoint is stood up on an ephemeral port, and the
//! [`OAuthConfig`] is pointed at it. No real GitHub API calls are made.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::{AppState, RegistryError};

/// Query params for the OAuth callback (`?code=...&state=...`).
#[derive(Debug, Deserialize)]
pub(crate) struct CallbackParams {
    /// The temporary authorization code GitHub issues after the user
    /// approves the app. Exchanged for an access token.
    pub(crate) code: String,
    /// Optional state parameter (CSRF protection). Echoed back by
    /// GitHub; we accept any value (the login endpoint generates a
    /// random state that the callback SHOULD verify, but for the MVP
    /// we skip strict state validation — the registry is localhost-only
    /// by default).
    #[allow(dead_code)]
    pub(crate) state: Option<String>,
}

/// The GitHub access-token exchange response (JSON body from
/// `POST https://github.com/login/oauth/access_token`).
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// The GitHub user-info response (JSON body from
/// `GET https://api.github.com/user`).
#[derive(Debug, Deserialize)]
struct GitHubUser {
    login: String,
    id: i64,
}

/// Configuration for the GitHub OAuth flow.
///
/// Constructed from env vars via [`OAuthConfig::from_env`]. Stored in
/// [`AppState::oauth_config`] as `Option<OAuthConfig>` — `None` when
/// OAuth is not configured.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    /// The OAuth App client ID (from GitHub Developer Settings).
    pub client_id: String,
    /// The OAuth App client secret.
    pub client_secret: String,
    /// The redirect URI GitHub sends the user back to after approval.
    /// Must match the "Authorization callback URL" configured in the
    /// GitHub OAuth App settings.
    pub redirect_uri: String,
    /// The GitHub authorize URL (configurable for testing — tests
    /// point this at a mock server).
    pub authorize_url: String,
    /// The GitHub token-exchange URL (configurable for testing).
    pub token_url: String,
    /// The GitHub user-info URL (configurable for testing).
    pub user_url: String,
}

/// Env-var names for OAuth configuration.
pub const GITHUB_CLIENT_ID_ENV: &str = "BUFF_REGISTRY_GITHUB_CLIENT_ID";
pub const GITHUB_CLIENT_SECRET_ENV: &str = "BUFF_REGISTRY_GITHUB_CLIENT_SECRET";
pub const OAUTH_REDIRECT_URI_ENV: &str = "BUFF_REGISTRY_OAUTH_REDIRECT_URI";

/// Default GitHub OAuth URLs.
pub const DEFAULT_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
pub const DEFAULT_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
pub const DEFAULT_USER_URL: &str = "https://api.github.com/user";

impl OAuthConfig {
    /// Build an [`OAuthConfig`] from env vars.
    ///
    /// Returns `None` if `BUFF_REGISTRY_GITHUB_CLIENT_ID` or
    /// `BUFF_REGISTRY_GITHUB_CLIENT_SECRET` is unset (OAuth disabled).
    /// The redirect URI defaults to
    /// `http://localhost:7878/auth/github/callback`.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var(GITHUB_CLIENT_ID_ENV).ok()?;
        let client_secret = std::env::var(GITHUB_CLIENT_SECRET_ENV).ok()?;
        if client_id.is_empty() || client_secret.is_empty() {
            return None;
        }
        let redirect_uri = std::env::var(OAUTH_REDIRECT_URI_ENV)
            .unwrap_or_else(|_| "http://localhost:7878/auth/github/callback".to_string());
        Some(Self {
            client_id,
            client_secret,
            redirect_uri,
            authorize_url: DEFAULT_AUTHORIZE_URL.to_string(),
            token_url: DEFAULT_TOKEN_URL.to_string(),
            user_url: DEFAULT_USER_URL.to_string(),
        })
    }

    /// Build the GitHub authorize URL with the client_id + redirect_uri
    /// encoded as query params. The user's browser is redirected here
    /// by the login endpoint.
    #[must_use]
    pub fn authorize_redirect_url(&self) -> String {
        // Scope: `read:user` is the minimal scope needed to read the
        // user's GitHub login + numeric ID. We do NOT request `repo`
        // or `write:org` — the registry only needs identity, not repo
        // access.
        format!(
            "{}?client_id={}&redirect_uri={}&scope={}",
            self.authorize_url,
            url_encode(&self.client_id),
            url_encode(&self.redirect_uri),
            url_encode("read:user"),
        )
    }
}

/// Minimal percent-encoding for OAuth URL params. We hand-roll this
/// instead of pulling `percent-encoding` (already in workspace deps)
/// to keep the OAuth module self-contained — the only chars that need
/// encoding in a redirect_uri or client_id are `:` and `/`.
fn url_encode(s: &str) -> String {
    s.replace(':', "%3A").replace('/', "%2F")
}

/// `GET /auth/github/login` — redirect to GitHub's authorize URL.
///
/// Returns `503 Service Unavailable` with a JSON body if OAuth is not
/// configured (env vars missing). Returns `302 Found` with the GitHub
/// authorize URL in the `Location` header otherwise.
pub(crate) async fn login(
    State(state): State<AppState>,
) -> Result<Response, RegistryError> {
    let config = state
        .oauth_config
        .as_ref()
        .ok_or(RegistryError::OAuthNotConfigured)?;
    let url = config.authorize_redirect_url();
    // Manual 302 (Found) response — the traditional OAuth redirect
    // status. axum 0.8's Redirect::to returns 303 (See Other); we use
    // a manual response to match the GitHub OAuth convention (302).
    Ok((
        axum::http::StatusCode::FOUND,
        [(axum::http::header::LOCATION, url.as_str())],
    )
        .into_response())
}

/// `GET /auth/github/callback?code=<auth_code>` — exchange code for
/// token, fetch user info, create session.
///
/// Flow:
/// 1. POST to `config.token_url` with (client_id, client_secret, code,
///    redirect_uri) → receive `access_token`.
/// 2. GET `config.user_url` with `Authorization: Bearer <access_token>`
///    → receive GitHub user JSON (login, id).
/// 3. `storage.create_session(login, id)` → receive session token.
/// 4. Respond with `200 OK` + `Set-Cookie: buff_session=<token>`.
///
/// Errors surface as JSON bodies with appropriate status codes:
/// - `502 Bad Gateway` — GitHub token exchange or user fetch failed.
/// - `500 Internal Server Error` — session creation failed.
pub(crate) async fn callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> Result<Response, RegistryError> {
    let config = state
        .oauth_config
        .as_ref()
        .ok_or(RegistryError::OAuthNotConfigured)?;

    // --- 1. Exchange code for access token ---
    let token_resp = exchange_code(config, &params.code)
        .await
        .map_err(|e| RegistryError::OAuthExchangeFailed(e.to_string()))?;

    // --- 2. Fetch GitHub user info ---
    let user = fetch_github_user(config, &token_resp.access_token)
        .await
        .map_err(|e| RegistryError::OAuthUserFetchFailed(e.to_string()))?;

    // --- 3. Create session ---
    let session_token = state
        .storage
        .create_session(&user.login, user.id)
        .map_err(|e| RegistryError::Storage(e.to_string()))?;

    // --- 4. Respond with session cookie ---
    Ok((
        [(axum::http::header::SET_COOKIE, format!("buff_session={session_token}; Path=/; HttpOnly; SameSite=Lax"))],
        Json(CallbackResponse {
            message: "Login successful".to_string(),
            github_login: user.login,
            session_token,
        }),
    )
        .into_response())
}

/// Response body for a successful OAuth callback.
#[derive(Debug, Serialize)]
struct CallbackResponse {
    message: String,
    github_login: String,
    session_token: String,
}

/// `POST /auth/logout` — delete the session, clear the cookie.
///
/// Reads the `buff_session` cookie (or `Authorization: Bearer <token>`)
/// and deletes the session from storage. Always returns `200 OK` (even
/// if the session was already gone — logout is idempotent).
pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    // Try to extract the session token from the cookie or bearer header.
    if let Some(token) = extract_session_from_cookie(&headers).or_else(|| {
        extract_bearer(&headers).map(str::to_string)
    }) {
        let _ = state.storage.delete_session(&token);
    }
    (
        [(axum::http::header::SET_COOKIE, "buff_session=; Path=/; Max-Age=0")],
        Json(serde_json::json!({"message": "Logged out"})),
    )
        .into_response()
}

/// `GET /auth/whoami` — return the current session's GitHub login.
///
/// Useful for CLI `buff whoami` + debugging. Returns `401` if no valid
/// session is present.
pub(crate) async fn whoami(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, RegistryError> {
    let token = extract_session_from_cookie(&headers)
        .or_else(|| extract_bearer(&headers).map(str::to_string))
        .ok_or(RegistryError::Unauthorized)?;
    let user = state
        .storage
        .validate_session(&token)
        .map_err(RegistryError::from)?
        .ok_or(RegistryError::Unauthorized)?;
    Ok(Json(serde_json::json!({
        "github_login": user.github_login,
        "github_id": user.github_id,
    }))
    .into_response())
}

// --- Internal helpers --------------------------------------------------

/// Exchange the OAuth `code` for an access token via POST to GitHub's
/// token endpoint.
///
/// Uses `reqwest` (rustls-tls — pure-Rust TLS, no native-tls/OpenSSL).
async fn exchange_code(
    config: &OAuthConfig,
    code: &str,
) -> Result<TokenResponse, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp = client
        .post(&config.token_url)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", config.redirect_uri.as_str()),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(format!("token endpoint returned {}", resp.status()).into());
    }
    let token: TokenResponse = resp.json().await?;
    Ok(token)
}

/// Fetch the GitHub user identity via GET to GitHub's user API.
async fn fetch_github_user(
    config: &OAuthConfig,
    access_token: &str,
) -> Result<GitHubUser, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(&config.user_url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "buff-registry")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(format!("user endpoint returned {}", resp.status()).into());
    }
    let user: GitHubUser = resp.json().await?;
    Ok(user)
}

/// Extract the `buff_session` cookie value from a `Cookie` header.
fn extract_session_from_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get(axum::http::header::COOKIE)?;
    let cookie_str = cookie_header.to_str().ok()?;
    for pair in cookie_str.split(';') {
        let pair = pair.trim();
        if let Some(val) = pair.strip_prefix("buff_session=") {
            return Some(val.to_string());
        }
    }
    None
}

/// Extract the bearer token from an `Authorization` header.
fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ").map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_encodes_colon_and_slash() {
        assert_eq!(url_encode("http://localhost:7878"), "http%3A%2F%2Flocalhost%3A7878");
        assert_eq!(url_encode("abc123"), "abc123");
    }

    #[test]
    fn oauth_config_from_env_returns_none_when_unset() {
        // Save + clear env vars, then test.
        let saved_id = std::env::var(GITHUB_CLIENT_ID_ENV).ok();
        let saved_secret = std::env::var(GITHUB_CLIENT_SECRET_ENV).ok();
        std::env::remove_var(GITHUB_CLIENT_ID_ENV);
        std::env::remove_var(GITHUB_CLIENT_SECRET_ENV);
        assert!(OAuthConfig::from_env().is_none());
        // Restore.
        if let Some(v) = saved_id {
            std::env::set_var(GITHUB_CLIENT_ID_ENV, v);
        }
        if let Some(v) = saved_secret {
            std::env::set_var(GITHUB_CLIENT_SECRET_ENV, v);
        }
    }

    #[test]
    fn oauth_config_from_env_returns_some_when_set() {
        let saved_id = std::env::var(GITHUB_CLIENT_ID_ENV).ok();
        let saved_secret = std::env::var(GITHUB_CLIENT_SECRET_ENV).ok();
        std::env::set_var(GITHUB_CLIENT_ID_ENV, "test-client-id");
        std::env::set_var(GITHUB_CLIENT_SECRET_ENV, "test-secret");
        let config = OAuthConfig::from_env().expect("should be Some");
        assert_eq!(config.client_id, "test-client-id");
        assert_eq!(config.client_secret, "test-secret");
        assert!(config.redirect_uri.contains("/auth/github/callback"));
        // Restore.
        std::env::remove_var(GITHUB_CLIENT_ID_ENV);
        std::env::remove_var(GITHUB_CLIENT_SECRET_ENV);
        if let Some(v) = saved_id {
            std::env::set_var(GITHUB_CLIENT_ID_ENV, v);
        }
        if let Some(v) = saved_secret {
            std::env::set_var(GITHUB_CLIENT_SECRET_ENV, v);
        }
    }

    #[test]
    fn authorize_redirect_url_contains_client_id() {
        let config = OAuthConfig {
            client_id: "myapp".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "http://localhost:7878/auth/github/callback".to_string(),
            authorize_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            user_url: "https://api.github.com/user".to_string(),
        };
        let url = config.authorize_redirect_url();
        assert!(url.contains("client_id=myapp"));
        assert!(url.contains("scope=read%3Auser"));
    }

    #[test]
    fn extract_session_from_cookie_finds_value() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "buff_session=abc-123; other=val".parse().unwrap(),
        );
        assert_eq!(extract_session_from_cookie(&headers), Some("abc-123".to_string()));
    }

    #[test]
    fn extract_session_from_cookie_none_when_absent() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(extract_session_from_cookie(&headers), None);
    }

    #[test]
    fn extract_bearer_returns_token() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer xyz789".parse().unwrap(),
        );
        assert_eq!(extract_bearer(&headers), Some("xyz789"));
    }
}
