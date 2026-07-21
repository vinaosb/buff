//! Spy handles — observe calls without changing dispatch behavior.
//!
//! A spy is obtained via [`Mock::spy`](crate::Mock::spy):
//!
//! ```ignore
//! let spy = mock.spy("greet");
//! mock.record_call("greet", vec![ArgumentValue::String("alice".into())]);
//! assert_eq!(spy.calls().len(), 1);
//! assert_eq!(spy.args()[0][0], ArgumentValue::String("alice".into()));
//! ```
//!
//! [`SpyHandle`] borrows the [`MockState`](crate::MockState) for the
//! lifetime of the borrow — it cannot outlive the mock. This is the
//! common-case ergonomics (no `Arc` clone, no lifetime juggling at the
//! mock site). For cross-thread or longer-lived spies, clone the mock
//! itself (`mock.clone()`) and call [`Mock::calls_for`](crate::Mock::calls_for).
//!
//! # Spies vs expectations
//!
//! Spies DO NOT add a [`Times`](crate::times::Times) constraint.
//! Calling [`Mock::spy`](crate::Mock::spy) does not affect verify
//! results. Spies are purely an observation handle.

use crate::record::{ArgumentValue, CallRecord};
use crate::state::MockState;

/// A handle for observing calls to a single method on a mock.
///
/// Construct via [`Mock::spy`](crate::Mock::spy). Borrows the
/// [`MockState`] — see the [module docs](self) for lifetime semantics.
pub struct SpyHandle<'a> {
    state: &'a MockState,
    method: String,
}

impl<'a> SpyHandle<'a> {
    /// Construct a spy handle bound to `state`, observing `method`.
    /// Used by [`Mock::spy`](crate::Mock::spy) — users do not call this
    /// directly.
    #[must_use]
    pub fn new(state: &'a MockState, method: impl Into<String>) -> Self {
        Self {
            state,
            method: method.into(),
        }
    }

    /// Snapshot of every call to this spy's method, in invocation order.
    /// Clones the records under the lock — safe to inspect without
    /// holding the mutex.
    #[must_use]
    pub fn calls(&self) -> Vec<CallRecord> {
        self.state.calls_for(&self.method)
    }

    /// Count of calls to this spy's method. Skips the `Vec` allocation
    /// of [`calls`](Self::calls).
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.state.call_count_for(&self.method)
    }

    /// Snapshot of just the argument lists of every call, dropping the
    /// method name (which is constant — it's this spy's `method`).
    /// Convenience for the common assertion pattern:
    /// `assert_eq!(spy.args(), vec![vec![ArgumentValue::String("a".into())]])`.
    #[must_use]
    pub fn args(&self) -> Vec<Vec<ArgumentValue>> {
        self.state
            .calls_for(&self.method)
            .into_iter()
            .map(|r| r.args)
            .collect()
    }
}

impl<'a> std::fmt::Debug for SpyHandle<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpyHandle")
            .field("method", &self.method)
            .field("observed_count", &self.call_count())
            .finish_non_exhaustive()
    }
}

