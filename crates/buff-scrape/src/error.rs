//! Error type for the `buff-scrape` crate.
//!
//! All fallible operations surface as [`ScrapeError`]. The underlying
//! `scraper::SelectorError` / `reqwest::Error` / `std::io::Error`
//! are mapped into this enum so the crate's public surface depends
//! only on `buff-scrape`'s own types (Buff code never sees a raw
//! `scraper::*` / `reqwest::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-scrape`
/// operation.
#[derive(Debug, Error)]
pub enum ScrapeError {
    /// A CSS selector failed to parse. The original message is
    /// carried verbatim (from `scraper::selector::SelectorError`)
    /// so a future `BuffError` migration can wrap it.
    #[error("invalid CSS selector: {0}")]
    Selector(String),

    /// The underlying `reqwest` crate returned an error (network
    /// failure, DNS resolution failure, TLS error, timeout, etc.).
    /// The original message is carried verbatim.
    #[error("HTTP error: {0}")]
    Http(String),

    /// The user supplied an empty string where HTML or a URL was
    /// required. Distinct from [`Self::Selector`] / [`Self::Http`]
    /// so the diagnostic can be specific.
    #[error("empty input")]
    EmptyInput,

    /// The user supplied a malformed URL. The original message is
    /// carried verbatim.
    #[error("invalid URL: {0}")]
    Url(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: scraper operation panicked")]
    Panic,
}

impl From<reqwest::Error> for ScrapeError {
    fn from(err: reqwest::Error) -> Self {
        ScrapeError::Http(err.to_string())
    }
}

impl From<url::ParseError> for ScrapeError {
    fn from(err: url::ParseError) -> Self {
        ScrapeError::Url(err.to_string())
    }
}
