//! Error type for the `buff-web` crate.
//!
//! All fallible operations surface as [`WebError`]. The HTTP layer maps
//! every error variant to a fixed HTTP status code via the
//! [`axum::response::IntoResponse`] impl on [`WebError`].
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points (`Web::listen` /
//! `Web::run`) wrap their bodies in `catch_unwind` so panics never
//! propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-web` operation.
#[derive(Debug, Error)]
pub enum WebError {
    /// Filesystem / network I/O failure (TCP bind failure, port in use,
    /// connection reset, etc.). Wraps the underlying [`std::io::Error`].
    #[error("web I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The user supplied a malformed bind address (empty string, invalid
    /// port, missing colon, etc.). The original parse message is carried
    /// verbatim so a future `BuffError` migration can wrap it.
    #[error("invalid bind address: {0}")]
    InvalidAddress(String),

    /// The user supplied a malformed route path. axum 0.8 requires paths
    /// starting with `/`; this variant fires for empty paths, paths
    /// missing the leading slash, or paths containing forbidden characters.
    #[error("invalid route path: {0}")]
    InvalidPath(String),

    /// A request body could not be read as UTF-8 text (binary payloads
    /// need a different reader — `Request.body_bytes` once added).
    /// Distinct from [`Self::Json`] so the diagnostic can be specific.
    #[error("request body is not valid UTF-8")]
    BodyNotUtf8,

    /// A JSON serialization or deserialization failed. Wraps the
    /// underlying `serde_json::Error` message verbatim.
    #[error("JSON error: {0}")]
    Json(String),

    /// The tokio runtime could not be constructed (usually: resource
    /// exhaustion / OS thread limit). Distinct from [`Self::Io`] so
    /// the diagnostic can reference the runtime layer specifically.
    #[error("failed to create tokio runtime")]
    RuntimeCreate,

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: web operation panicked")]
    Panic,
}

impl From<serde_json::Error> for WebError {
    fn from(err: serde_json::Error) -> Self {
        WebError::Json(err.to_string())
    }
}

impl From<axum::http::header::InvalidHeaderName> for WebError {
    fn from(err: axum::http::header::InvalidHeaderName) -> Self {
        WebError::InvalidPath(format!("invalid header name: {err}"))
    }
}

impl From<axum::http::header::InvalidHeaderValue> for WebError {
    fn from(err: axum::http::header::InvalidHeaderValue) -> Self {
        WebError::InvalidPath(format!("invalid header value: {err}"))
    }
}
