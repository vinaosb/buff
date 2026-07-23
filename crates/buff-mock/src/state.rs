//! Shared, interior-mutable mock state.
//!
//! [`MockState`] is the single source of truth for a mock instance: the
//! programmed expectations, the recorded calls, and the active spies.
//! Both [`Mock<T>`](crate::Mock) and [`SpyHandle`](crate::SpyHandle)
//! hold an [`std::sync::Arc<MockState>`] — so a spy obtained before a
//! call sees the call after it returns (no copy/snapshotting).
//!
//! # Thread-safety
//!
//! Interior mutability via [`std::sync::Mutex`]. Every public method
//! takes `&self` and acquires the lock internally. The lock is NEVER
//! held across a user-provided closure (it's dropped before any
//! potentially-panicking user code runs), so a panic in user code
//! cannot poison the mutex via the recording path.
//!
//! Poisoning (impossible in normal operation — but possible if a
//! downstream user wraps the mock) is surfaced as
//! [`MockError::Poisoned`](crate::MockError::Poisoned) via the
//! `PoisonError` -> `MockError` `From` impl in [`error`].

use std::sync::Mutex;

use crate::error::{MockError, MockResult};
use crate::expectation::Expectation;
use crate::record::CallRecord;

/// The shared state backing a [`Mock<T>`](crate::Mock).
///
/// Stored inside an `Arc<MockState>` so the mock itself, spy handles,
/// and any codegen-emitted trait impls can all observe the same state.
/// Public methods take `&self` and acquire the interior lock — callers
/// never see [`std::sync::MutexGuard`] directly.
///
/// # Determinism
///
/// Internal collections are `Vec`s (insertion-ordered) — no
/// [`std::collections::HashMap`] / [`std::collections::HashSet`]
/// anywhere in this struct (project hard rule). Expectations are
/// scanned linearly in insertion order; the first match wins.
pub struct MockState {
    /// Every expectation programmed via
    /// [`Mock::expect`](crate::Mock::expect). Scanned in insertion order
    /// during dispatch + verify.
    expectations: Mutex<Vec<Expectation>>,
    /// Every recorded call, in invocation order. Pushed during dispatch
    /// BEFORE the programmed return value is produced (so a panic in
    /// `returning` cannot lose the record).
    calls: Mutex<Vec<CallRecord>>,
    /// The set of method names that have an active spy (via
    /// [`Mock::spy`](crate::Mock::spy)). Spies don't change dispatch
    /// behavior — they only enable [`SpyHandle::calls`] to filter the
    /// shared call log to the spy's method.
    spied_methods: Mutex<Vec<String>>,
}

impl MockState {
    /// Construct empty mock state (no expectations, no calls, no spies).
    /// Used by [`Mock::<T>::new`](crate::Mock::new).
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            expectations: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            spied_methods: Mutex::new(Vec::new()),
        }
    }

    /// Push a programmed expectation. Called by
    /// [`ExpectationBuilder::build`](crate::ExpectationBuilder) (the
    /// terminal step of an `expect(...)` chain).
    pub(crate) fn add_expectation(&self, expectation: Expectation) {
        if let Ok(mut guard) = self.expectations.lock() {
            guard.push(expectation);
        }
    }

    /// Register a spy on `method`. Returns immediately — the spy
    /// observes future calls via [`Self::calls_for`]. Re-registering
    /// the same method is a no-op (the spy is idempotent).
    pub(crate) fn add_spy(&self, method: &str) {
        if let Ok(mut guard) = self.spied_methods.lock() {
            if !guard.iter().any(|m| m == method) {
                guard.push(method.to_string());
            }
        }
    }

    /// Record a dispatch. Called by the codegen-emitted trait impl
    /// (or by the user's manual mock impl) BEFORE the programmed return
    /// value is consulted. This ensures a panic in the return-value
    /// lookup cannot lose the call record.
    pub(crate) fn record_call(&self, record: CallRecord) {
        if let Ok(mut guard) = self.calls.lock() {
            guard.push(record);
        }
    }

    /// Look up the programmed return value for a call to `method` with
    /// `args`. The first expectation (in insertion order) whose method
    /// name matches AND whose argument constraint matches (or has no
    /// argument constraint) is consulted. Returns [`None`] when no
    /// expectation matches (the trait impl must then provide a default
    /// value or surface [`MockError::UnexpectedCall`]).
    #[must_use]
    pub(crate) fn lookup_return(
        &self,
        method: &str,
        args: &[crate::record::ArgumentValue],
    ) -> Option<crate::record::ReturnValue> {
        let guard = self.expectations.lock().ok()?;
        for exp in guard.iter() {
            if exp.matches(method, args) {
                return exp.return_value.clone();
            }
        }
        None
    }

    /// Snapshot of every recorded call, in invocation order.
    ///
    /// Returns a fresh `Vec` (cloned under the lock) so callers can
    /// inspect the records without holding the mutex. Empty if no
    /// calls have been recorded (or if the mutex is poisoned — cannot
    /// happen in normal operation).
    #[must_use]
    pub(crate) fn calls(&self) -> Vec<CallRecord> {
        self.calls.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Snapshot of calls to a single method (the spy accessor).
    /// Filters the shared call log to records whose `method` field
    /// matches `method_name`. Used by [`SpyHandle::calls`].
    #[must_use]
    pub(crate) fn calls_for(&self, method_name: &str) -> Vec<CallRecord> {
        self.calls
            .lock()
            .map(|g| {
                g.iter()
                    .filter(|r| r.method == method_name)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Count of calls to a single method. Equivalent to
    /// `self.calls_for(method).len()` but skips the `Vec` allocation.
    #[must_use]
    pub(crate) fn call_count_for(&self, method_name: &str) -> usize {
        self.calls
            .lock()
            .map(|g| g.iter().filter(|r| r.method == method_name).count())
            .unwrap_or(0)
    }

    /// Drop every recorded call + expectation + spy registration.
    /// Useful when a test reuses a mock across multiple sub-assertions
    /// and wants to reset to a clean slate between them.
    ///
    /// No-op on a poisoned mutex (cannot happen in normal operation).
    pub(crate) fn clear(&self) {
        if let Ok(mut e) = self.expectations.lock() {
            e.clear();
        }
        if let Ok(mut c) = self.calls.lock() {
            c.clear();
        }
        if let Ok(mut s) = self.spied_methods.lock() {
            s.clear();
        }
    }

    /// Verify all programmed expectations against the observed calls.
    ///
    /// Walks every expectation and checks its [`Times`] constraint
    /// against the count of calls to its method. Returns
    /// [`MockError::VerifyFailed`] enumerating every failure (one
    /// compound error message — keeps the API simple).
    ///
    /// Pure: does not mutate state. Acquires the expectations + calls
    /// locks sequentially (never nested — see the thread-safety note
    /// on [`MockState`]).
    pub(crate) fn verify(&self) -> MockResult<()> {
        let expectations = self.expectations.lock().map_err(MockError::from)?.clone();
        let calls = self.calls.lock().map_err(MockError::from)?;

        let mut failures: Vec<String> = Vec::new();
        for exp in &expectations {
            let observed = calls.iter().filter(|r| r.method == exp.method).count();
            if !exp.times.matches(observed) {
                failures.push(format!(
                    "`{}` expected {}, got {}",
                    exp.method,
                    exp.times.describe(),
                    observed
                ));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(MockError::VerifyFailed(failures.join("; ")))
        }
    }

    /// Total number of programmed expectations. Used by tests to assert
    /// the `expect()` chain was registered.
    #[must_use]
    pub(crate) fn expectation_count(&self) -> usize {
        self.expectations.lock().map(|g| g.len()).unwrap_or(0)
    }
}

impl Default for MockState {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MockState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let exp_count = self.expectation_count();
        let call_count = self.calls.lock().map(|g| g.len()).unwrap_or(0);
        f.debug_struct("MockState")
            .field("expectations", &exp_count)
            .field("calls", &call_count)
            .finish_non_exhaustive()
    }
}
