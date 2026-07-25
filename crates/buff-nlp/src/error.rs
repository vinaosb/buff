//! Error type for the `buff-nlp` crate.
//!
//! All fallible operations surface as [`NlpError`]. The public entry
//! points ([`crate::Text::stem`]) wrap the underlying
//! `rust-stemmers` crate (which no longer exposes an `Error` type —
//! `Stemmer::create` returns a `Stemmer` directly) so the public
//! surface depends only on `buff-nlp`'s own types (Buff code never
//! sees a raw `rust_stemmers::*` / `whatlang::*` /
//! `unicode_segmentation::*` error type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-nlp` operation.
#[derive(Debug, Error)]
pub enum NlpError {
    /// The underlying `rust-stemmers` crate failed to construct a
    /// stemmer for the requested algorithm. Covers: unsupported
    /// algorithm variant (defensive — every public [`crate::StemAlgorithm`]
    /// variant maps to a known-good algorithm internally) or an
    /// internal allocation / initialization failure. The original
    /// message is carried verbatim so a future `BuffError` migration
    /// can wrap it.
    #[error("stemmer init error for {algorithm:?}: {message}")]
    StemmerInit {
        algorithm: crate::StemAlgorithm,
        message: String,
    },

    /// The user supplied an empty string to an operation that requires
    /// non-empty input (e.g. [`crate::Text::stem`] on an empty word).
    /// Distinct from [`Self::StemmerInit`] so the diagnostic can be
    /// specific.
    #[error("nlp operation called with empty input")]
    EmptyInput,

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: nlp operation panicked")]
    Panic,
}
