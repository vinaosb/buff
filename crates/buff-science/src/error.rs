//! Error types for buff-science operations.

use std::fmt;

/// The error type for buff-science operations.
#[derive(Debug, Clone, PartialEq)]
pub enum ScienceError {
    /// The input is not a matrix (wrong rank).
    NotMatrix(usize),
    /// The matrix is not square.
    NotSquare { rows: usize, cols: usize },
    /// The matrix is singular (non-invertible).
    SingularMatrix,
    /// Shape mismatch between operands.
    ShapeMismatch { lhs: Vec<usize>, rhs: Vec<usize> },
    /// The input data is empty.
    Empty,
    /// Length mismatch between slices.
    LengthMismatch { expected: usize, got: usize },
    /// Value out of interpolation range.
    OutOfRange { x: f64, min: f64, max: f64 },
    /// Gradient descent did not converge.
    ConvergenceFailed { steps: usize },
    /// Numerical error during computation.
    NumericalError(String),
}

impl fmt::Display for ScienceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScienceError::NotMatrix(rank) => {
                write!(f, "expected rank-2 matrix, got rank {rank}")
            }
            ScienceError::NotSquare { rows, cols } => {
                write!(f, "expected square matrix, got {rows}x{cols}")
            }
            ScienceError::SingularMatrix => write!(f, "matrix is singular"),
            ScienceError::ShapeMismatch { lhs, rhs } => {
                write!(f, "shape mismatch: {:?} vs {:?}", lhs, rhs)
            }
            ScienceError::Empty => write!(f, "input is empty"),
            ScienceError::LengthMismatch { expected, got } => {
                write!(f, "length mismatch: expected {expected}, got {got}")
            }
            ScienceError::OutOfRange { x, min, max } => {
                write!(f, "value {x} out of range [{min}, {max}]")
            }
            ScienceError::ConvergenceFailed { steps } => {
                write!(f, "convergence failed after {steps} steps")
            }
            ScienceError::NumericalError(msg) => {
                write!(f, "numerical error: {msg}")
            }
        }
    }
}

impl std::error::Error for ScienceError {}

/// A convenience alias for `Result<T, ScienceError>`.
pub type ScienceResult<T> = Result<T, ScienceError>;
