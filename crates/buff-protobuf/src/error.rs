//! Error type for the `buff-protobuf` crate.
//!
//! All fallible operations surface as [`ProtobufError`]. The public
//! [`crate::Message::new`] / [`crate::Message::from_bytes`] /
//! [`crate::serialize`] / [`crate::deserialize`] entry points map the
//! underlying `prost::DecodeError` / `prost::EncodeError` into this
//! enum so the crate's public surface depends only on `buff-protobuf`'s
//! own types (Buff code never sees a raw `prost::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-protobuf` operation.
#[derive(Debug, Error)]
pub enum ProtobufError {
    /// The underlying `prost` encode operation failed. Covers buffer
    /// capacity issues and (rarely) oversized varints. The original
    /// message is carried verbatim.
    #[error("protobuf encode error: {0}")]
    Encode(String),

    /// The underlying `prost` decode operation failed. Covers
    /// malformed wire-format bytes, truncated input, unexpected
    /// wire-type tags, and over-long varints.
    #[error("protobuf decode error: {0}")]
    Decode(String),

    /// The user supplied a number that is not finite (NaN / ±Infinity)
    /// when encoding. protobuf's `Value::number_value` is a finite
    /// `f64` — JSON's wider number space is rejected so the round-trip
    /// is well-defined.
    #[error("protobuf cannot encode non-finite number: {0}")]
    NonFiniteNumber(f64),

    /// The user supplied an empty byte slice to [`crate::deserialize`]
    /// or [`crate::Message::from_bytes`]. Distinct from
    /// [`Self::Decode`] (which fires for non-empty but malformed
    /// bytes) so the diagnostic can be specific.
    #[error("protobuf deserialize called with empty buffer")]
    EmptyBuffer,

    /// A field name in a decoded [`prost_types::Struct`] was not valid
    /// UTF-8. This is rare (protobuf field names are ASCII) but the
    /// fallback ensures the boundary never panics on hostile input.
    #[error("protobuf field name was not valid UTF-8: {0}")]
    BadUtf8(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: protobuf operation panicked")]
    Panic,
}

impl From<prost::EncodeError> for ProtobufError {
    fn from(err: prost::EncodeError) -> Self {
        ProtobufError::Encode(err.to_string())
    }
}

impl From<prost::DecodeError> for ProtobufError {
    fn from(err: prost::DecodeError) -> Self {
        ProtobufError::Decode(err.to_string())
    }
}

impl From<std::string::FromUtf8Error> for ProtobufError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        ProtobufError::BadUtf8(err.to_string())
    }
}

impl From<std::str::Utf8Error> for ProtobufError {
    fn from(err: std::str::Utf8Error) -> Self {
        ProtobufError::BadUtf8(err.to_string())
    }
}
