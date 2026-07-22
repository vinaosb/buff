use thiserror::Error;

/// The single error type returned by fallible `buff-fake` operations.
#[derive(Debug, Error)]
pub enum FakerError {
    /// The datetime range was invalid (start after end, or unparseable).
    #[error("invalid date range: {0}")]
    InvalidDateRange(String),

    /// A wrapper-internal panic was caught by `catch_unwind`.
    #[error("internal error: fake operation panicked")]
    Panic,
}
