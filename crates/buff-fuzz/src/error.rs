//! Error model for `buff-fuzz`.
//!
//! [`FuzzError`] is the crate-local error enum (derived via `thiserror`,
//! mirroring buff-mock / buff-dap / buff-eval precedent). [`FuzzResult`]
//! is the standard `Result` alias. The runner uses the
//! [`FuzzError::PropertyFailed`] variant to surface a single failed
//! property check; multiple failures accumulate in [`FuzzSummary`] and
//! surface as a single error at the end of the run.

use std::fmt;

/// The canonical `Result` alias for fallible `buff-fuzz` operations.
pub type FuzzResult<T> = Result<T, FuzzError>;

/// Errors raised by the `buff-fuzz` runtime.
///
/// Every variant carries enough context for a developer to diagnose
/// the failure without re-running — the failing input, the strategy
/// that produced it, or the count of accumulated property failures.
#[derive(Debug, thiserror::Error)]
pub enum FuzzError {
    /// The user-supplied property closure returned `false` for `input`.
    ///
    /// Used internally to record a single failure during a [`crate::run`]
    /// pass; the default runner accumulates these in [`FuzzSummary`]
    /// rather than short-circuiting on the first failure.
    #[error("property failed for input {input}")]
    PropertyFailed { input: i64 },

    /// The strategy cannot drive a property (e.g. an empty `Int` range
    /// where `min > max`, or a `String` strategy with `max_len == 0`).
    ///
    /// Returned by [`crate::Strategy`] constructors and by [`crate::run`]
    /// when the strategy is structurally invalid.
    #[error("invalid strategy: {reason}")]
    InvalidStrategy { reason: String },

    /// The iteration count was zero. [`crate::run`] requires at least
    /// one iteration to be meaningful.
    #[error("iteration count must be > 0, got {count}")]
    InvalidIterations { count: u32 },

    /// The codegen helper [`crate::lower_fuzz_harness`] could not lower
    /// the supplied Buff function — e.g. unsupported parameter type,
    /// missing parameter, or a multi-parameter function (MVP supports
    /// single-arg `@fuzz func name(input: Int)` only).
    #[error("lowering failed for fuzz target `{fn_name}`: {reason}")]
    LoweringFailed { fn_name: String, reason: String },
}

impl FuzzError {
    /// Construct an `InvalidStrategy` error with the supplied reason.
    pub(crate) fn invalid_strategy(reason: impl Into<String>) -> Self {
        Self::InvalidStrategy {
            reason: reason.into(),
        }
    }

    /// Construct an `InvalidIterations` error with the supplied count.
    pub(crate) fn invalid_iterations(count: u32) -> Self {
        Self::InvalidIterations { count }
    }

    /// Construct a `LoweringFailed` error with the supplied fn name + reason.
    pub(crate) fn lowering_failed(fn_name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::LoweringFailed {
            fn_name: fn_name.into(),
            reason: reason.into(),
        }
    }
}

/// A display-friendly accumulator for multiple `FuzzError::PropertyFailed`
/// values. Used internally by the [`crate::run`] runner to summarise N
/// failures as a single error message; the public surface returns
/// [`FuzzSummary`] instead.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FailureBatch {
    /// The inputs that triggered property failures, in observation order.
    pub inputs: Vec<i64>,
}

impl FailureBatch {
    /// Construct an empty batch.
    pub(crate) fn new() -> Self {
        Self { inputs: Vec::new() }
    }

    /// Record a new failing input.
    pub(crate) fn push(&mut self, input: i64) {
        self.inputs.push(input);
    }

    /// Number of failures accumulated so far.
    pub fn len(&self) -> usize {
        self.inputs.len()
    }

    /// Whether the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }
}

impl fmt::Display for FailureBatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failure(s)", self.inputs.len())?;
        if !self.inputs.is_empty() {
            let preview: Vec<String> = self.inputs.iter().take(3).map(|i| i.to_string()).collect();
            write!(f, ": [{}]", preview.join(", "))?;
            if self.inputs.len() > 3 {
                write!(f, ", ...")?;
            }
        }
        Ok(())
    }
}
