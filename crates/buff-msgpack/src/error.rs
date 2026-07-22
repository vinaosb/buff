//! Error type for the `buff-msgpack` crate.
//!
//! All fallible operations surface as [`MsgPackError`]. The public
//! [`crate::serialize`] / [`crate::deserialize`] entry points map the
//! underlying `rmp_serde::encode::Error` / `rmp_serde::decode::Error`
//! into this enum so the crate's public surface depends only on
//! `buff-msgpack`'s own types.

use thiserror::Error;

/// The single error type returned by every fallible `buff-msgpack` operation.
#[derive(Debug, Error)]
pub enum MsgPackError {
    /// The underlying `rmp_serde` encode operation failed.
    #[error("msgpack encode error: {0}")]
    Encode(String),

    /// The underlying `rmp_serde` decode operation failed.
    #[error("msgpack decode error: {0}")]
    Decode(String),

    /// The user supplied an empty byte slice to [`crate::deserialize`].
    #[error("msgpack deserialize called with empty buffer")]
    EmptyBuffer,

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: msgpack operation panicked")]
    Panic,
}

impl From<rmp_serde::encode::Error> for MsgPackError {
    fn from(err: rmp_serde::encode::Error) -> Self {
        MsgPackError::Encode(err.to_string())
    }
}

impl From<rmp_serde::decode::Error> for MsgPackError {
    fn from(err: rmp_serde::decode::Error) -> Self {
        MsgPackError::Decode(err.to_string())
    }
}
