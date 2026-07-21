//! Errors raised by `buff-tensor` operations.
//!
//! Single enum [`TensorError`] wrapping the small set of failure modes the
//! MVP surface exposes. Mirrors the workspace pattern
//! (`thiserror::Error` derive, no `unwrap`/`expect`/`panic!` in non-test
//! code, every fallible op returns `Result<_, TensorError>`).

use thiserror::Error;

/// Error type for all `buff-tensor` operations.
///
/// Variants follow the precedent set by `buff-lang-runtime::RuntimeError`
/// (thiserror derive, `#[error]` messages phrased as user-facing
/// diagnostics, no internal state exposed).
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TensorError {
    /// Shape mismatch on binary op (elementwise, matmul). Carries the
    /// two incompatible shapes for diagnostics.
    #[error("shape mismatch: lhs={lhs:?} rhs={rhs:?}")]
    ShapeMismatch {
        /// LHS shape (the receiver of the binary op).
        lhs: Vec<usize>,
        /// RHS shape (the argument).
        rhs: Vec<usize>,
    },
    /// A user-supplied index was out of bounds for the tensor's shape.
    /// Carries the offending index and the shape for diagnostics.
    #[error("index out of bounds: index={index:?} shape={shape:?}")]
    IndexOutOfBounds {
        /// The offending multi-dimensional index.
        index: Vec<usize>,
        /// The tensor shape against which the index was checked.
        shape: Vec<usize>,
    },
    /// The tensor's rank (ndim) is wrong for the operation. Carries the
    /// actual rank and (when relevant) the expected rank.
    #[error("rank mismatch: actual={actual} expected={expected}")]
    RankMismatch {
        /// Actual rank of the tensor.
        actual: usize,
        /// Expected rank (operation-specific).
        expected: usize,
    },
    /// A reshape target shape's element count does not match the source.
    #[error("reshape element count mismatch: got={got} target={target}")]
    ReshapeMismatch {
        /// Source element count (the actual data length).
        got: usize,
        /// Target element count (the requested shape's product).
        target: usize,
    },
    /// A data buffer's length does not match the shape's element count.
    #[error("data length mismatch: data_len={data_len} shape_elements={shape_elements}")]
    DataLengthMismatch {
        /// Actual length of the supplied data buffer.
        data_len: usize,
        /// Element count implied by the shape.
        shape_elements: usize,
    },
    /// Rank exceeds the MVP cap (4). Carries the offending rank.
    /// Per T8 spec: "Do NOT support rank > 4 (defer to v1.18+)".
    #[error("rank exceeds MVP cap of 4: rank={0} (deferred to v1.18+)")]
    RankTooLarge(usize),
    /// Reduction axis is out of bounds for the tensor's rank.
    #[error("axis out of bounds: axis={axis} rank={rank}")]
    AxisOutOfBounds {
        /// The offending axis.
        axis: isize,
        /// The tensor's rank.
        rank: usize,
    },
    /// Empty tensor passed to an operation that requires at least one
    /// element (e.g. `max`).
    #[error("empty tensor: operation requires at least one element")]
    Empty,
    /// Generic broadcast failure for ops that could support broadcasting
    /// in the future. Reserved for forward-compat.
    #[error("broadcast not supported for shapes: lhs={lhs:?} rhs={rhs:?}")]
    BroadcastUnsupported {
        /// LHS shape.
        lhs: Vec<usize>,
        /// RHS shape.
        rhs: Vec<usize>,
    },
}

/// Convenience alias so callers write `Result<Tensor<f32>>` instead of
/// `Result<Tensor<f32>, TensorError>`.
pub type TensorResult<T> = Result<T, TensorError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_shape_mismatch() {
        let err = TensorError::ShapeMismatch {
            lhs: vec![2, 3],
            rhs: vec![3, 2],
        };
        assert_eq!(
            err.to_string(),
            "shape mismatch: lhs=[2, 3] rhs=[3, 2]"
        );
    }

    #[test]
    fn error_display_index_out_of_bounds() {
        let err = TensorError::IndexOutOfBounds {
            index: vec![5],
            shape: vec![3],
        };
        assert_eq!(
            err.to_string(),
            "index out of bounds: index=[5] shape=[3]"
        );
    }

    #[test]
    fn error_display_rank_too_large() {
        let err = TensorError::RankTooLarge(5);
        assert_eq!(
            err.to_string(),
            "rank exceeds MVP cap of 4: rank=5 (deferred to v1.18+)"
        );
    }

    #[test]
    fn error_clone_eq() {
        // Errors derive Clone + PartialEq so tests can assert on them.
        let a = TensorError::Empty;
        let b = a.clone();
        assert_eq!(a, b);
    }
}
