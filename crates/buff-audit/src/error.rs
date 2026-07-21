//! Error type for the `buff-audit` crate.
//!
//! Every fallible operation surfaces as [`AuditError`]. Wraps the
//! underlying `ed25519_dalek::SignatureError` + `hex::FromHexError`
//! into Buff's R3 error-mapping contract (no raw Rust error type
//! crosses the FFI boundary).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-audit` operation.
#[derive(Debug, Error)]
pub enum AuditError {
    /// The user supplied an invalid Ed25519 signature (wrong length,
    /// bad encoding, or failed verification). The original message is
    /// carried verbatim so a future `BuffError` migration can wrap it.
    #[error("invalid signature: {0}")]
    BadSignature(String),

    /// The user supplied an invalid Ed25519 public or secret key
    /// (wrong length, bad encoding). Distinct from [`Self::BadSignature`]
    /// so the diagnostic can be specific.
    #[error("invalid key: {0}")]
    BadKey(String),

    /// The user supplied invalid hex input (odd length, non-ASCII
    /// hex chars, etc.). Wraps the underlying `hex::FromHexError`.
    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),

    /// Filesystem I/O failure while reading the dep manifest or
    /// walking a project tree (file not found, permission denied).
    /// Wraps the underlying [`std::io::Error`].
    #[error("audit I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: audit operation panicked")]
    Panic,
}

impl From<ed25519_dalek::SignatureError> for AuditError {
    fn from(err: ed25519_dalek::SignatureError) -> Self {
        AuditError::BadSignature(err.to_string())
    }
}
