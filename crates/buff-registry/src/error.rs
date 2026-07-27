//! Error types for the Buff registry.
//!
//! [`RegistryError`] is the single domain error returned by HTTP handlers.
//! It maps to HTTP status codes via its [`axum::response::IntoResponse`]
//! impl — the response body is a JSON object `{"error": "<message>"}` so
//! clients can surface a human-readable string.
//!
//! [`StorageError`] is the lower-level error returned by [`crate::Storage`]
//! implementations. Handlers convert it to [`RegistryError::Storage`]
//! (which renders as HTTP 500). The in-memory storage impl essentially
//! never fails (no IO), but the trait keeps the failure surface explicit
//! so a real database-backed impl can drop in later.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

/// A domain error returned by HTTP handlers.
///
/// Each variant maps to a fixed HTTP status code (see [`IntoResponse`]).
/// The `Display` form (via `thiserror`) is the human-readable message
/// returned to the client in the JSON `error` field.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Package name failed validation (empty, path separator, `..`,
    /// non-`[a-z0-9_-]`, or length out of range). HTTP 400.
    #[error("invalid package name")]
    InvalidName,

    /// Version string failed `semver::Version::parse`. HTTP 400.
    #[error("invalid version: {0}")]
    InvalidVersion(String),

    /// Request body failed JSON parsing or missed a required field.
    /// HTTP 400.
    #[error("invalid request body: {0}")]
    InvalidBody(String),

    /// Publish attempted to overwrite an already-published `(name, version)`
    /// pair. HTTP 400 (the registry is append-only for v1.6 — no
    /// republish / yank yet).
    #[error("version already exists: {name}@{version}")]
    VersionExists {
        /// The package name.
        name: String,
        /// The already-published version string.
        version: String,
    },

    /// Publish attempted to introduce a dependency cycle (publishing
    /// `A` with a transitive dep back on `A`). HTTP 409 Conflict.
    #[error("cycle detected")]
    CycleDetected,

    /// Missing or malformed `Authorization: Bearer <token>` header, OR
    /// the supplied token is unknown to the storage. HTTP 401.
    #[error("unauthorized")]
    Unauthorized,

    /// The token has exceeded its per-window publish budget. HTTP 429.
    #[error("rate limit exceeded")]
    RateLimited,

    /// `GET /api/v1/package/<name>` for an unknown package. HTTP 404.
    #[error("package not found")]
    NotFound,

    /// `GET /api/v1/download/<name>/<version>` for an unknown version
    /// (the package exists, but the requested version doesn't).
    /// HTTP 404.
    #[error("version not found")]
    VersionNotFound,

    /// `GET /api/v1/resolve/<name>?req=...` returned no matching
    /// version. HTTP 404.
    #[error("no version matching requirement")]
    NoMatchingVersion,

    /// The storage layer returned a failure (mutex poisoning in the
    /// in-memory impl; DB / blob-store failure in a future impl).
    /// HTTP 500.
    #[error("storage error: {0}")]
    Storage(String),

    /// The publish body's `tarball_b64` field failed base64 decoding.
    /// HTTP 400.
    #[error("invalid tarball encoding: {0}")]
    InvalidTarball(String),

    // --- T57: OAuth + production errors ---
    /// T57: GitHub OAuth is not configured (env vars missing).
    /// HTTP 503 Service Unavailable.
    #[error("GitHub OAuth is not configured. Set BUFF_REGISTRY_GITHUB_CLIENT_ID and BUFF_REGISTRY_GITHUB_CLIENT_SECRET to enable login.")]
    OAuthNotConfigured,

    /// T57: The GitHub token-exchange step failed (network error,
    /// invalid code, rate-limited by GitHub, etc.). HTTP 502.
    #[error("GitHub token exchange failed: {0}")]
    OAuthExchangeFailed(String),

    /// T57: The GitHub user-info fetch failed. HTTP 502.
    #[error("GitHub user fetch failed: {0}")]
    OAuthUserFetchFailed(String),

    /// P0.25 (sec-003): The OAuth `state` parameter is missing or does
    /// not match the value bound to the login redirect via the
    /// `buff_oauth_state` cookie. Indicates a CSRF attempt or a stale
    /// / replayed callback URL. HTTP 400 Bad Request.
    #[error("OAuth state parameter missing or mismatched (possible CSRF attempt)")]
    OAuthStateMismatch,

    /// T57: The GitHub user is not on the invite-only beta allowlist.
    /// HTTP 403 Forbidden.
    #[error("Buff registry is in invite-only beta. Your GitHub account is not on the allowlist.")]
    NotAllowlisted,

    /// T57: The authenticated user does not have permission to publish
    /// to the requested scope (e.g. not a member of `@org`). HTTP 403.
    #[error("you do not have permission to publish to this scope")]
    ScopeForbidden,

    /// P0.28 (sec-hardening): A user-supplied input failed one of the
    /// handler-level validation helpers in [`crate::handlers`]:
    /// strict package-name regex, semver shape, path-traversal /
    /// null-byte / absolute-path defense. The `String` carries a
    /// human-readable reason ("invalid package name: must start with
    /// lowercase letter a-z (got 'T')") so the client can surface a
    /// useful error. HTTP 400 Bad Request.
    ///
    /// This is distinct from [`RegistryError::InvalidName`] (static
    /// "invalid package name" message, retained for backwards compat)
    /// and [`RegistryError::InvalidVersion`] (semver-parse-failure
    /// message). `InvalidInput` carries the validator's exact
    /// rejection reason for any input kind (name, version, path).
    #[error("{0}")]
    InvalidInput(String),
}

impl IntoResponse for RegistryError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::InvalidName
            | Self::InvalidVersion(_)
            | Self::InvalidBody(_)
            | Self::VersionExists { .. }
            | Self::InvalidTarball(_)
            | Self::InvalidInput(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::CycleDetected => StatusCode::CONFLICT,
            Self::NotFound | Self::VersionNotFound | Self::NoMatchingVersion => {
                StatusCode::NOT_FOUND
            }
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::OAuthNotConfigured => StatusCode::SERVICE_UNAVAILABLE,
            Self::OAuthExchangeFailed(_) | Self::OAuthUserFetchFailed(_) => StatusCode::BAD_GATEWAY,
            Self::OAuthStateMismatch => StatusCode::BAD_REQUEST,
            Self::NotAllowlisted | Self::ScopeForbidden => StatusCode::FORBIDDEN,
        };
        let body = Json(ErrorBody {
            error: self.to_string(),
        });
        (status, body).into_response()
    }
}

impl From<StorageError> for RegistryError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value.to_string())
    }
}

/// A storage-layer failure returned by [`crate::Storage`] implementations.
///
/// The in-memory impl essentially only fails on mutex poisoning; a real
/// DB-backed impl would surface connection errors, serialization errors,
/// etc. via the same type.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Lower-level storage failure (mutex poisoned, IO error, etc.).
    #[error("storage failure: {0}")]
    Failure(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}
