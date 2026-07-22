//! Error type for the `buff-http-client` crate.
//!
//! All fallible operations surface as [`HttpError`]. The underlying
//! `reqwest::Error` is mapped into this enum so the crate's public
//! surface depends only on `buff-http-client`'s own types (Buff code
//! never sees a raw `reqwest::*` type).

use thiserror::Error;

/// The single error type returned by every fallible `buff-http-client`
/// operation.
#[derive(Debug, Error)]
pub enum HttpError {
    /// The underlying `reqwest` crate returned an error (network failure,
    /// DNS resolution failure, TLS error, timeout, etc.). The original
    /// message is carried verbatim so a future `BuffError` migration can
    /// wrap it.
    #[error("HTTP error: {0}")]
    Request(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: HTTP client panicked")]
    Panic,
}

impl From<reqwest::Error> for HttpError {
    fn from(err: reqwest::Error) -> Self {
        HttpError::Request(err.to_string())
    }
}
