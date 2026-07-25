//! Error type for the `buff-auth` crate.
//!
//! Every fallible operation surfaces as [`AuthError`]. Wraps the
//! underlying `jsonwebtoken::errors::Error` +
//! `argon2::password_hash::Error` + `oauth2::RequestTokenError` +
//! `reqwest::Error` + `serde_json::Error` into Buff's R3 error-mapping
//! contract (no raw Rust error type crosses the FFI boundary).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-auth` operation.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The user supplied an invalid JWT (bad signature, expired,
    /// malformed). The original message is carried verbatim so a
    /// future `BuffError` migration can wrap it.
    #[error("jwt error: {0}")]
    Jwt(String),

    /// The user supplied an invalid password hash (PHC string
    /// malformed, wrong algorithm, hash mismatch). Distinct from
    /// [`Self::Jwt`] so the diagnostic can be specific.
    #[error("password hash error: {0}")]
    PasswordHash(String),

    /// The password hash did not match the supplied plaintext
    /// password. Returns `Ok(false)` at the wrapper layer instead of
    /// an error (mirrors the T26 Signature.verify stance — a
    /// verification failure is NOT an error). This variant is
    /// reserved for hash-format failures (the wrapper falls back to
    /// `Ok(false)` on plain hash mismatch).
    #[error("password hash mismatch")]
    PasswordMismatch,

    /// The OAuth2 client encountered an error: bad auth URL, code
    /// exchange failed, token endpoint returned an error response,
    /// or HTTP transport failure. The original message is carried
    /// verbatim.
    #[error("oauth2 error: {0}")]
    OAuth2(String),

    /// The user supplied an invalid RBAC policy entry (duplicate
    /// role, malformed triple, etc.). The Rbac policy is owned by
    /// the user (not the framework); construction never panics.
    #[error("rbac policy error: {0}")]
    Rbac(String),

    /// JSON (de)serialisation failure — used internally for shuffling
    /// JWT claims between `Map<String, Unknown>` and serde_json::Value.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: auth operation panicked")]
    Panic,
}

impl From<jsonwebtoken::errors::Error> for AuthError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        AuthError::Jwt(err.to_string())
    }
}

impl From<argon2::password_hash::Error> for AuthError {
    fn from(err: argon2::password_hash::Error) -> Self {
        AuthError::PasswordHash(err.to_string())
    }
}

// `oauth2::RequestTokenError` is generic over both the HTTP transport
// error (`RE`) AND the OAuth2 error-response body (`T`) in oauth2 4.4+.
// A blanket impl keeps the public surface independent of the underlying
// `T` (BasicErrorResponse vs a future custom variant).
impl<RE, T> From<oauth2::RequestTokenError<RE, T>> for AuthError
where
    RE: std::error::Error + Send + Sync + 'static,
    T: oauth2::ErrorResponse + Send + Sync + 'static,
{
    fn from(err: oauth2::RequestTokenError<RE, T>) -> Self {
        AuthError::OAuth2(err.to_string())
    }
}

impl From<reqwest::Error> for AuthError {
    fn from(err: reqwest::Error) -> Self {
        AuthError::OAuth2(format!("http transport: {err}"))
    }
}
