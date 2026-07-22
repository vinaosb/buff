//! Error type for the `buff-resilience` crate.
//!
//! All fallible operations surface as [`ResilienceError`]. Each primitive
//! (Retry / CircuitBreaker / RateLimiter / Timeout / Pipeline) maps its
//! failure mode into this enum so the crate's public surface depends only
//! on `buff-resilience`'s own types (Buff code never sees a raw underlying
//! error type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use std::time::Duration;

use thiserror::Error;

/// The single error type returned by every fallible `buff-resilience`
/// operation.
#[derive(Debug, Error)]
pub enum ResilienceError {
    /// The retry policy exhausted all `max_attempts` without success.
    /// The last error message is carried verbatim so a future
    /// `BuffError` migration can wrap it. `attempts` is the total
    /// number of attempts made (>= 1).
    #[error("retry exhausted after {attempts} attempt(s); last error: {last_error}")]
    Exhausted { attempts: u32, last_error: String },

    /// The circuit breaker is currently open and refused to dispatch
    /// the call. Distinct from [`Self::Exhausted`] so callers can
    /// distinguish "fail-fast because we know it's broken" from
    /// "we tried and the upstream kept failing".
    #[error("circuit breaker is open (failure_count={failure_count}, threshold={threshold})")]
    CircuitOpen { failure_count: u32, threshold: u32 },

    /// The rate limiter has no tokens available right now and the
    /// caller chose not to block. Returned by
    /// [`crate::RateLimiter::try_execute`]; the blocking
    /// [`crate::RateLimiter::execute`] waits instead.
    #[error("rate limit exceeded ({requests_per_second} req/s)")]
    RateLimited { requests_per_second: f64 },

    /// The operation did not complete within the configured timeout.
    /// The configured duration is carried for the diagnostic.
    #[error("operation timed out after {0:?}")]
    Timeout(Duration),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: resilience operation panicked")]
    Panic,
}
