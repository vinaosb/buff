//! Backoff strategies for retry scheduling.
//!
//! [`Backoff`] describes how the delay between retries grows as the
//! attempt counter increases. The three variants cover the schedules
//! every major job-queue framework ships (Celery / Bull / asynq /
//! Quartz / Hangfire): fixed, linear, exponential.
//!
//! # Hard-rule compliance
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` in this module.
//! All constructors are infallible at the surface; structurally
//! invalid schedules (e.g. zero base delay for exponential) are
//! surfaced as [`crate::JobsError::InvalidBackoff`] at the
//! [`Backoff::delay`] call site.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::JobsError;

/// The maximum delay cap (24 hours) so an exponential backoff can
/// never overflow `Duration` or stall a worker indefinitely.
const MAX_DELAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A retry-backoff schedule.
///
/// Constructed via the type-level associated functions
/// ([`Backoff::fixed`], [`Backoff::linear`],
/// [`Backoff::exponential`]). The delay for the Nth retry is
/// computed via [`Backoff::delay`].
///
/// `attempt` is 1-indexed: `delay(1)` is the delay BEFORE the first
/// retry (i.e. after the initial execution failed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Backoff {
    /// Constant delay between retries. `delay(N) == base` for every N.
    Fixed { base: Duration },

    /// Delay grows linearly: `delay(N) == base * N`.
    Linear { base: Duration },

    /// Delay grows exponentially: `delay(N) == min(base * 2^(N-1), max)`.
    /// The `max` cap prevents runaway delays on jobs with very high
    /// `max_retries`.
    Exponential { base: Duration, max: Duration },
}

impl Backoff {
    /// Constant delay between retries. `base` must be non-zero (a
    /// zero base returns [`JobsError::InvalidBackoff`] at the delay
    /// call site, not at construction - mirrors the buff-fuzz
    /// Strategy pattern of infallible constructors).
    pub fn fixed(base: Duration) -> Self {
        Self::Fixed { base }
    }

    /// Linearly growing delay: `delay(N) == base * N`.
    pub fn linear(base: Duration) -> Self {
        Self::Linear { base }
    }

    /// Exponentially growing delay capped at `max`:
    /// `delay(N) == min(base * 2^(N-1), max)`.
    pub fn exponential(base: Duration, max: Duration) -> Self {
        Self::Exponential { base, max }
    }

    /// Compute the delay before the Nth retry (1-indexed).
    ///
    /// `attempt == 0` returns [`JobsError::InvalidAttempt`] (the 0th
    /// execution is the initial try, not a retry). `attempt >
    /// max_retries` is a programming error and also returns
    /// `InvalidAttempt`; the worker checks the budget before asking
    /// for a delay.
    pub fn delay(&self, attempt: u32, max_retries: u32) -> Result<Duration, JobsError> {
        if attempt == 0 {
            return Err(JobsError::InvalidAttempt {
                attempt,
                max_retries,
            });
        }
        if attempt > max_retries {
            return Err(JobsError::InvalidAttempt {
                attempt,
                max_retries,
            });
        }
        Ok(match self {
            Self::Fixed { base } => *base,
            Self::Linear { base } => base
                .checked_mul(attempt)
                .unwrap_or(MAX_DELAY)
                .min(MAX_DELAY),
            Self::Exponential { base, max } => {
                if base.is_zero() {
                    return Err(JobsError::invalid_backoff(
                        "exponential base delay must be > 0",
                    ));
                }
                let shift = attempt.saturating_sub(1);
                if shift >= 31 {
                    return Ok(*max);
                }
                let factor = 1u64 << shift;
                base.checked_mul(factor as u32)
                    .map(|d| d.min(*max).min(MAX_DELAY))
                    .unwrap_or(*max)
            }
        })
    }
}

impl Default for Backoff {
    /// Default backoff: fixed 1-second delay between retries. The
    /// conservative default matches Celery / Bull / asynq defaults.
    fn default() -> Self {
        Self::Fixed {
            base: Duration::from_secs(1),
        }
    }
}

impl std::fmt::Display for Backoff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fixed { base } => write!(f, "Backoff.fixed({:?})", base),
            Self::Linear { base } => write!(f, "Backoff.linear({:?})", base),
            Self::Exponential { base, max } => {
                write!(f, "Backoff.exponential({:?}, max={:?})", base, max)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_delay_is_constant() {
        let b = Backoff::fixed(Duration::from_millis(500));
        assert_eq!(b.delay(1, 5).unwrap(), Duration::from_millis(500));
        assert_eq!(b.delay(5, 5).unwrap(), Duration::from_millis(500));
    }

    #[test]
    fn linear_delay_grows() {
        let b = Backoff::linear(Duration::from_millis(100));
        assert_eq!(b.delay(1, 5).unwrap(), Duration::from_millis(100));
        assert_eq!(b.delay(3, 5).unwrap(), Duration::from_millis(300));
        assert_eq!(b.delay(5, 5).unwrap(), Duration::from_millis(500));
    }

    #[test]
    fn exponential_delay_doubles() {
        let b = Backoff::exponential(Duration::from_millis(100), Duration::from_secs(60));
        assert_eq!(b.delay(1, 10).unwrap(), Duration::from_millis(100));
        assert_eq!(b.delay(2, 10).unwrap(), Duration::from_millis(200));
        assert_eq!(b.delay(3, 10).unwrap(), Duration::from_millis(400));
        assert_eq!(b.delay(4, 10).unwrap(), Duration::from_millis(800));
    }

    #[test]
    fn exponential_delay_caps_at_max() {
        let b = Backoff::exponential(Duration::from_secs(1), Duration::from_secs(8));
        assert_eq!(b.delay(10, 10).unwrap(), Duration::from_secs(8));
    }

    #[test]
    fn delay_rejects_attempt_zero() {
        let b = Backoff::fixed(Duration::from_secs(1));
        assert!(b.delay(0, 5).is_err());
    }

    #[test]
    fn delay_rejects_attempt_exceeding_max() {
        let b = Backoff::fixed(Duration::from_secs(1));
        assert!(b.delay(6, 5).is_err());
    }

    #[test]
    fn exponential_rejects_zero_base() {
        let b = Backoff::exponential(Duration::ZERO, Duration::from_secs(10));
        assert!(b.delay(1, 5).is_err());
    }
}
