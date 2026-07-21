//! Programmed expectations: what the mock should return and how many
//! times each method should be called.
//!
//! # Builder pattern
//!
//! [`ExpectationBuilder`] is the user-facing fluent API:
//!
//! ```ignore
//! mock.expect("greet").returning(ReturnValue::String("hi".into())).times(1);
//! ```
//!
//! The builder holds a `&mut MockState` (or rather, an `Arc` clone plus
//! the method name) and pushes a fully-formed [`Expectation`] into the
//! shared state when the builder is dropped or when a terminal method
//! (`returning`, `times`) is called.
//!
//! # Argument matching
//!
//! Optional — `expect("greet")` with no `.with_args(...)` matches ANY
//! call to `greet`. When `.with_args([String("alice")])` is set, the
//! expectation only fires for calls whose captured args are exactly
//! equal (position + value).

use crate::record::{ArgumentValue, ReturnValue};
use crate::state::MockState;
use crate::times::Times;

/// One programmed expectation.
///
/// Built via [`ExpectationBuilder`] and pushed into [`MockState`] at
/// the end of the chain. Stored in a `Vec<Expectation>` so dispatch
/// scans insertion order — first match wins.
#[derive(Debug, Clone)]
pub struct Expectation {
    /// The method name this expectation targets (`"greet"`, `"compute"`).
    pub method: String,
    /// Optional exact-match argument constraint. When `None`, the
    /// expectation matches any call to `method`. When `Some`, matches
    /// only calls whose captured `args` are exactly equal (position +
    /// value).
    pub args_constraint: Option<Vec<ArgumentValue>>,
    /// Programmed return value. When `None`, the dispatch returns the
    /// type's default (and a verify-time error if no default exists).
    pub return_value: Option<ReturnValue>,
    /// Call-count constraint. Defaults to [`Times::Any`].
    pub times: Times,
}

impl Expectation {
    /// Construct a bare expectation targeting `method`, no arg
    /// constraint, no return value, [`Times::Any`] count. Used by
    /// [`ExpectationBuilder::new`].
    #[must_use]
    pub(crate) fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            args_constraint: None,
            return_value: None,
            times: Times::default(),
        }
    }

    /// Returns `true` if this expectation matches a call to `method`
    /// with `args`. Matching is method-name + (optional) arg-equality.
    /// Pure function — used by [`MockState::lookup_return`] and verify.
    #[must_use]
    pub(crate) fn matches(&self, method: &str, args: &[ArgumentValue]) -> bool {
        if self.method != method {
            return false;
        }
        match &self.args_constraint {
            None => true,
            Some(required) => required.len() == args.len()
                && required.iter().zip(args.iter()).all(|(r, a)| r == a),
        }
    }
}

impl PartialEq for Expectation {
    /// Custom `PartialEq` — compares every field except the borrow
    /// implied by the builder (which doesn't appear in the struct).
    fn eq(&self, other: &Self) -> bool {
        self.method == other.method
            && self.args_constraint == other.args_constraint
            && self.return_value == other.return_value
            && self.times == other.times
    }
}

/// Builder returned by [`Mock::expect`](crate::Mock::expect).
///
/// Fluent API: each chainable method consumes `self` and returns a new
/// builder with the corresponding field updated. Terminal methods
/// (`returning`, `times`, `at_least`, etc.) push the built expectation
/// into the shared [`MockState`] and return `()`.
///
/// # Multiple expectations on the same method
///
/// Calling `expect("greet")` twice produces TWO expectations on
/// `greet`. Dispatch checks them in insertion order; verify checks
/// both. This is the idiomatic way to express "return X for the first
/// call, then Y for subsequent calls" (the second expectation sets
/// `times: Any` and `return_value: Y`).
pub struct ExpectationBuilder<'a> {
    state: &'a MockState,
    expectation: Expectation,
}

impl<'a> ExpectationBuilder<'a> {
    /// Construct a builder targeting `method` against `state`. Pushed
    /// into `state` when [`Self::returning`] or another terminal
    /// method is called. Used by [`Mock::expect`](crate::Mock::expect).
    #[must_use]
    pub(crate) fn new(state: &'a MockState, method: impl Into<String>) -> Self {
        Self {
            state,
            expectation: Expectation::new(method),
        }
    }

    /// Constrain this expectation to calls whose arguments are exactly
    /// equal (position + value) to `args`. Consumes `self` and returns
    /// the builder so the chain can continue with `returning` / `times`.
    #[must_use]
    pub fn with_args(mut self, args: Vec<ArgumentValue>) -> Self {
        self.expectation.args_constraint = Some(args);
        self
    }

    /// Program the return value AND commit the expectation to the
    /// shared state. The terminal call of the `expect(...)` chain.
    pub fn returning(self, value: ReturnValue) {
        let mut e = self.expectation;
        e.return_value = Some(value);
        self.state.add_expectation(e);
    }

    /// Program the return value, set `times: Exact(n)`, and commit.
    /// Convenience for the common `expect(...).returning(v).times(n)`
    /// triple.
    /// Commit the expectation with `times: Exact(n)` and no return
    /// value. Useful when the mock should accept the call but the
    /// trait method's default return is fine.
    pub fn times(mut self, n: usize) {
        self.expectation.times = Times::Exact(n);
        self.state.add_expectation(self.expectation);
    }

    /// Commit with `times: AtLeast(min)`.
    pub fn at_least(mut self, min: usize) {
        self.expectation.times = Times::AtLeast(min);
        self.state.add_expectation(self.expectation);
    }

    /// Commit with `times: AtMost(max)`.
    pub fn at_most(mut self, max: usize) {
        self.expectation.times = Times::AtMost(max);
        self.state.add_expectation(self.expectation);
    }

    /// Commit with `times: Never`. The mock will FAIL verify if the
    /// method is invoked even once.
    pub fn never(mut self) {
        self.expectation.times = Times::Never;
        self.state.add_expectation(self.expectation);
    }
}

