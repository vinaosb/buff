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
}

impl IntoResponse for RegistryError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::InvalidName
            | Self::InvalidVersion(_)
            | Self::InvalidBody(_)
            | Self::VersionExists { .. }
            | Self::InvalidTarball(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::CycleDetected => StatusCode::CONFLICT,
            Self::NotFound | Self::VersionNotFound | Self::NoMatchingVersion => {
                StatusCode::NOT_FOUND
            }
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
