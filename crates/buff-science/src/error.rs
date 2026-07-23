//! Errors raised by `buff-science` operations.
//!
//! Single enum [`ScienceError`] wrapping the failure modes the MVP surface
//! exposes. Mirrors the workspace pattern (`thiserror::Error` derive, no
//! `unwrap`/`expect`/`panic!` in non-test code).

use thiserror::Error;

/// Error type for all `buff-science` operations.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ScienceError {
    /// Matrix is singular (non-invertible).
    #[error("singular matrix: determinant is zero or near-zero")]
    SingularMatrix,

    /// Shape mismatch for the operation.
    #[error("shape mismatch: lhs={lhs:?} rhs={rhs:?}")]
    ShapeMismatch {
        /// LHS shape.
        lhs: Vec<usize>,
        /// RHS shape.
        rhs: Vec<usize>,
    },

    /// Matrix must be square for this operation.
    #[error("matrix must be square: got {rows}x{cols}")]
    NotSquare {
        /// Number of rows.
        rows: usize,
        /// Number of columns.
        cols: usize,
    },

    /// Rank must be 2 for matrix operations.
    #[error("expected rank 2 (matrix), got rank {0}")]
    NotMatrix(usize),

    /// Empty input where at least one element is required.
    #[error("empty input: operation requires at least one element")]
    Empty,

    /// Interpolation x is out of the defined range.
    #[error("interpolation x={x} out of range [{min}, {max}]")]
    OutOfRange {
        /// The x value attempted.
        x: f64,
        /// Minimum x in the dataset.
        min: f64,
        /// Maximum x in the dataset.
        max: f64,
    },

    /// Mismatched input lengths.
    #[error("length mismatch: expected {expected}, got {got}")]
    LengthMismatch {
        /// Expected length.
        expected: usize,
        /// Actual length.
        got: usize,
    },

    /// Numerical convergence failure.
    #[error("convergence failed after {steps} steps")]
    ConvergenceFailed {
        /// Number of steps attempted.
        steps: usize,
    },
}

/// Convenience alias so callers write `Result<T>` instead of
/// `Result<T, ScienceError>`.
pub type ScienceResult<T> = Result<T, ScienceError>;
