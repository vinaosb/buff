//! Error type for the `buff-config` crate.
//!
//! All fallible operations surface as [`ConfigError`]. The single
//! public entry point maps the underlying `figment::Error` into this
//! enum so the crate's public surface depends only on `buff-config`'s
//! own types (Buff code never sees a raw `figment::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-config` operation.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// A figment-level error (parse failure, type mismatch, missing
    /// required key, etc.). The original message is carried verbatim
    /// so a future `BuffError` migration can wrap it.
    #[error("config error: {0}")]
    Figment(String),

    /// Filesystem I/O failure (file not found, permission denied, etc.).
    /// Wraps the underlying [`std::io::Error`].
    #[error("config I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: config operation panicked")]
    Panic,
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        ConfigError::Figment(err.to_string())
    }
}
