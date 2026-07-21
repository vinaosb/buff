//! The generic `Mock<T>` wrapper — a trait-erased mock instance.
//!
//! `Mock<T>` holds an [`Arc<MockState>`](std::sync::Arc) and a
//! [`PhantomData<T>`](std::marker::PhantomData). The trait `T` itself
//! is NOT implemented by `Mock<T>` — that's the job of the
//! codegen-emitted (or user-written) trait impl, which delegates to
//! [`Mock::<T>::record_call`] + [`Mock::<T>::lookup_return`].
//!
//! # Why not auto-derive the trait impl here
//!
//! Rust has no way to auto-implement an arbitrary user trait for a
//! generic wrapper without procedural macros (forbidden per T3 spike
//! DEFER). The codegen-time helper [`lower_mock_for_trait`](crate::lower_mock_for_trait)
//! emits the trait impl as `syn::Item`s, ready for the next compilation
//! pass to compile. This is the "codegen-time expansion" the T3 spike
//! recommends as the runtime workaround.
//!
//! # Send + Sync
//!
//! `Mock<T>` is `Send + Sync` regardless of `T` (the [`PhantomData`]
//! carries no ownership; the [`Arc<MockState>`](std::sync::Arc) is
//! `Send + Sync` because [`MockState`] uses [`std::sync::Mutex`] for
//! interior mutability).

use std::marker::PhantomData;
use std::sync::Arc;

use crate::error::MockResult;
use crate::expectation::ExpectationBuilder;
use crate::record::{ArgumentValue, CallRecord, ReturnValue};
use crate::spy::SpyHandle;
use crate::state::MockState;

/// A mock instance for trait `T`.
///
/// Holds shared state via [`Arc<MockState>`](std::sync::Arc) — clones
/// share the same expectations + call log. Construct via
/// [`Mock::<T>::new`](Self::new); program expectations via
/// [`expect`](Self::expect); verify via [`verify`](Self::verify).
///
/// See the [module docs](self) for why `Mock<T>` does not auto-impl `T`.
pub struct Mock<T: ?Sized> {
    state: Arc<MockState>,
    _marker: PhantomData<T>,
}

// Manual Clone — `T: Clone` is NOT required (only the Arc is cloned).
impl<T: ?Sized> Clone for Mock<T> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            _marker: PhantomData,
        }
    }
}

impl<T: ?Sized> Mock<T> {
    /// Construct an empty mock. No expectations, no recorded calls.
    ///
    /// The trait `T` is a phantom parameter — pass it explicitly:
    /// `Mock::<MyTrait>::new()`. In generated code (after
    /// `lower_mock_for_trait`), the trait impl's constructor wraps
    /// this.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(MockState::new()),
            _marker: PhantomData,
        }
    }

    /// Begin an `expect(method).returning(value)` chain. The returned
    /// [`ExpectationBuilder`] is consumed by a terminal method
    /// (`returning`, `times`, `at_least`, …) which commits the
    /// expectation to the shared state.
    #[must_use]
    pub fn expect(&self, method: &str) -> ExpectationBuilder<'_> {
        ExpectationBuilder::new(&self.state, method)
    }

    /// Register a spy on `method`. Returns a [`SpyHandle`] bound to
    /// this mock's shared state — every future call to `method` is
    /// observable via `spy.calls()`.
    ///
    /// Spying does NOT change dispatch behavior — the call still
    /// returns the programmed value (or default). Spying only enables
    /// post-hoc inspection.
    #[must_use]
    pub fn spy(&self, method: &str) -> SpyHandle<'_> {
        self.state.add_spy(method);
        SpyHandle::new(&self.state, method)
    }

    /// Verify all programmed expectations against the observed calls.
    /// Returns `Ok(())` if every [`Times`] constraint is satisfied,
    /// else a compound [`MockError::VerifyFailed`](crate::MockError::VerifyFailed)
    /// enumerating every failure.
    ///
    /// Idempotent: calling twice produces the same result (state is
    /// not mutated).
    pub fn verify(&self) -> MockResult<()> {
        self.state.verify()
    }

    /// Record a call to `method` with `args`. Called by the
    /// codegen-emitted trait impl BEFORE the programmed return value
    /// is consulted — so even if `lookup_return` later fails, the
    /// call was recorded.
    pub fn record_call(&self, method: &str, args: Vec<ArgumentValue>) {
        self.state.record_call(CallRecord {
            method: method.to_string(),
            args,
        });
    }

    /// Convenience: record a call with no arguments.
    pub fn record_call_no_args(&self, method: &str) {
        self.record_call(method, Vec::new());
    }

    /// Look up the programmed return value for a call to `method` with
    /// `args`. Returns [`None`] when no expectation matches — the
    /// codegen-emitted trait impl is then responsible for producing a
    /// default value or surfacing an error.
    #[must_use]
    pub fn lookup_return(
        &self,
        method: &str,
        args: &[ArgumentValue],
    ) -> Option<ReturnValue> {
        self.state.lookup_return(method, args)
    }

    /// Convenience: look up the return value for a no-argument call.
    /// Equivalent to `self.lookup_return(method, &[])`.
    #[must_use]
    pub fn lookup_return_no_args(&self, method: &str) -> Option<ReturnValue> {
        self.lookup_return(method, &[])
    }

    /// Snapshot of every recorded call, in invocation order. Clones
    /// the underlying `Vec` under the lock — safe to inspect without
    /// holding the mutex.
    #[must_use]
    pub fn calls(&self) -> Vec<CallRecord> {
        self.state.calls()
    }

    /// Count of calls to a single method. Skips the `Vec` allocation
    /// of an equivalent filtered scan over [`calls`](Self::calls).
    #[must_use]
    pub fn call_count_for(&self, method: &str) -> usize {
        self.state.call_count_for(method)
    }

    /// Drop every recorded call + every programmed expectation + every
    /// spy registration. Useful when a test reuses a mock across
    /// multiple sub-assertions and wants to reset to a clean slate.
    pub fn clear(&self) {
        self.state.clear();
    }
}

impl<T: ?Sized> Default for Mock<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ?Sized> std::fmt::Debug for Mock<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mock")
            .field("state", &self.state)
            .field("trait", &std::any::type_name::<T>())
            .finish_non_exhaustive()
    }
}

