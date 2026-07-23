//! Error type for `buff-ml`.
//!
//! All fallible operations return [`MlResult`]. Errors are derived via
//! `thiserror::Error` (mirrors the workspace-wide pattern in
//! `buff-tensor::error` / `buff-plugins::error`).

use thiserror::Error;

/// Result alias for all `buff-ml` fallible operations.
pub type MlResult<T> = std::result::Result<T, MlError>;

/// Errors raised by `buff-ml`.
///
/// Variants are additive only (mirrors the workspace stability stance for
/// error enums — new variants may be appended, existing ones are never
/// renumbered/removed). The [`thiserror::Error`] derive gives every variant
/// a stable `Display` rendering.
#[derive(Debug, Error)]
pub enum MlError {
    /// A shape mismatch between two operands (e.g. `Linear` forward where
    /// the input's last dim does not equal `input_dim`, or elementwise ops
    /// on tensors of different shapes).
    #[error("shape mismatch: {lhs:?} vs {rhs:?}")]
    ShapeMismatch {
        lhs: Vec<usize>,
        rhs: Vec<usize>,
    },

    /// A rank mismatch (e.g. `Linear` forward expects a rank-2 input).
    #[error("rank mismatch: expected rank {expected}, got {actual}")]
    RankMismatch {
        actual: usize,
        expected: usize,
    },

    /// A dimension was zero where a positive size is required.
    #[error("dimension {name} must be positive, got {value}")]
    InvalidDimension {
        name: &'static str,
        value: usize,
    },

    /// A probability argument was outside `[0, 1]` (e.g. `Dropout::new(-0.5)`).
    #[error("probability out of range [0, 1]: {value}")]
    InvalidProbability {
        value: f32,
    },

    /// A learning rate / scalar hyperparameter was non-finite or non-positive.
    #[error("invalid hyperparameter {name}: {value}")]
    InvalidHyperparameter {
        name: &'static str,
        value: f32,
    },

    /// The serialized model JSON was malformed or referenced an unknown
    /// layer kind.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// An underlying `buff-tensor` operation failed. Wrapped (not flattened)
    /// so the original `TensorError` detail is preserved for the caller.
    #[error("tensor error: {0}")]
    Tensor(#[from] buff_tensor::TensorError),

    /// An I/O error during `Model::save` / `Model::load`.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
