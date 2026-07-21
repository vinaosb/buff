//! Call-count expectations for [`Mock::expect`](crate::Mock::expect).
//!
//! A [`Times`] constraint says how many times a method MAY be invoked
//! before [`Mock::verify`](crate::Mock::verify) flags it. Every variant
//! is matched via [`Times::matches`](crate::Times::matches).
//!
//! # Why a dedicated enum (not just `usize`)
//!
//! Real-world mock usage frequently wants `at_least(1)` ("this method
//! was called at all") or `at_most(3)` ("no infinite loop"). Encoding
//! the constraint as an enum lets the verify pass produce a precise
//! error message (`expected at least 1 call, got 0`) without losing
//! information by collapsing to a single integer.

use std::fmt;

/// A call-count constraint.
///
/// Built via the [`ExpectationBuilder`](crate::ExpectationBuilder) chain
/// (`times`, `at_least`, `at_most`, `never`, `any`). Defaults to
/// [`Times::Any`] when no constraint is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Times {
    /// Exactly `n` calls. Constructed via `ExpectationBuilder::times(n)`.
    Exact(usize),
    /// `>= min` calls. Constructed via `ExpectationBuilder::at_least(min)`.
    AtLeast(usize),
    /// `<= max` calls. Constructed via `ExpectationBuilder::at_most(max)`.
    AtMost(usize),
    /// An inclusive range `[min, max]`. Constructed via
    /// `ExpectationBuilder::times_range(min, max)`. Stored separately
    /// from `Exact`/`AtLeast`/`AtMost` for clearer error messages.
    Range { min: usize, max: usize },
    /// Zero calls. Constructed via `ExpectationBuilder::never()`. The
    /// mock will FAIL `verify` if the method is invoked even once —
    /// useful for asserting a branch was NOT taken.
    Never,
    /// No constraint. The method may be called any number of times
    /// (including zero). The default when `expect(method)` is used
    /// without a follow-up `times` call.
    Any,
}

impl Times {
    /// Returns `true` if the observed call count satisfies this constraint.
    ///
    /// Pure function — no allocation, no I/O. Used by the verify pass.
    #[must_use]
    pub fn matches(&self, observed: usize) -> bool {
        match *self {
            Times::Exact(n) => observed == n,
            Times::AtLeast(min) => observed >= min,
            Times::AtMost(max) => observed <= max,
            Times::Range { min, max } => observed >= min && observed <= max,
            Times::Never => observed == 0,
            Times::Any => true,
        }
    }

    /// Human-readable rendering of the constraint, used in verify-failure
    /// messages (`expected exactly 2 calls`, `expected at most 1 call`,
    /// `expected never to be called`).
    #[must_use]
    pub(crate) fn describe(&self) -> String {
        match *self {
            Times::Exact(0) => "never to be called".to_string(),
            Times::Exact(1) => "exactly 1 call".to_string(),
            Times::Exact(n) => format!("exactly {n} calls"),
            Times::AtLeast(0) => "any number of calls (>= 0)".to_string(),
            Times::AtLeast(1) => "at least 1 call".to_string(),
            Times::AtLeast(n) => format!("at least {n} calls"),
            Times::AtMost(0) => "never to be called (at most 0)".to_string(),
            Times::AtMost(1) => "at most 1 call".to_string(),
            Times::AtMost(n) => format!("at most {n} calls"),
            Times::Range { min, max } => {
                format!("between {min} and {max} calls (inclusive)")
            }
            Times::Never => "never to be called".to_string(),
            Times::Any => "any number of calls".to_string(),
        }
    }
}

impl Default for Times {
    /// Defaults to [`Times::Any`] — the least restrictive constraint.
    fn default() -> Self {
        Times::Any
    }
}

impl fmt::Display for Times {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

