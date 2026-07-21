//! Error type for the `buff-cli` crate.
//!
//! All fallible operations surface as [`CliError`]. The single public
//! entry point [`crate::App::parse`] maps the underlying `clap::Error`
//! into this enum so the crate's public surface depends only on
//! `buff-cli`'s own types (Buff code never sees a raw `clap::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-cli` operation.
#[derive(Debug, Error)]
pub enum CliError {
    /// A clap-level parse error (unknown flag, missing required arg,
    /// invalid value, etc.). The original message is carried verbatim
    /// so a future `BuffError` migration can wrap it. Includes the
    /// generated help text when clap attaches one.
    #[error("CLI parse error: {0}")]
    Parse(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: CLI operation panicked")]
    Panic,
}

impl From<clap::Error> for CliError {
    fn from(err: clap::Error) -> Self {
        CliError::Parse(err.to_string())
    }
}
